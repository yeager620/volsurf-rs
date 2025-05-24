use eframe::egui;
use options_rs::api::{RestClient, WebSocketClient};
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::volatility::{ImpliedVolatility, VolatilitySurface};
use options_rs::models::OptionContract;
use options_rs::utils::{plot_volatility_smile, plot_volatility_surface};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

struct VolatilitySurfaceApp {
    ticker_input: String,
    status: String,
    ticker_sender: mpsc::Sender<String>,
}

impl eframe::App for VolatilitySurfaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Live Volatility Surface Plotter");

            ui.horizontal(|ui| {
                ui.label("Ticker Symbol:");
                ui.text_edit_singleline(&mut self.ticker_input);

                if ui.button("Plot Volatility Surface").clicked() {
                    if self.ticker_input.trim().is_empty() {
                        self.status = "Please enter a ticker symbol".to_string();
                    } else {
                        let ticker = self.ticker_input.trim().to_uppercase();
                        self.status =
                            format!("Starting volatility surface plotting for {}", ticker);

                        // Send the ticker to the plotting thread
                        if let Err(e) = self.ticker_sender.try_send(ticker) {
                            self.status = format!("Error: {}", e);
                        }
                    }
                }
            });

            ui.separator();
            ui.label(&self.status);
            ui.separator();
            ui.label("The volatility surface will be plotted in the output directory.");
            ui.label("Press Ctrl+C in the terminal to exit the application.");
        });
    }
}

pub fn parse_options_chain(data: &Value) -> Result<Vec<OptionContract>> {
    let mut options = Vec::new();

    if let Some(results) = data.get("results") {
        if let Some(results_array) = results.as_array() {
            for option_data in results_array {
                if let (Some(symbol), Some(option_type), Some(strike), Some(expiration)) = (
                    option_data.get("symbol").and_then(|s| s.as_str()),
                    option_data.get("option_type").and_then(|t| t.as_str()),
                    option_data.get("strike_price").and_then(|p| p.as_f64()),
                    option_data.get("expiration_date").and_then(|d| d.as_str()),
                ) {
                    if let Ok(exp_date) = chrono::DateTime::parse_from_rfc3339(expiration) {
                        let exp_utc = exp_date.with_timezone(&chrono::Utc);
                        let option_type = match option_type {
                            "call" => options_rs::models::OptionType::Call,
                            "put" => options_rs::models::OptionType::Put,
                            _ => continue,
                        };

                        let contract =
                            OptionContract::new(symbol.to_string(), option_type, strike, exp_utc);

                        options.push(contract);
                    }
                }
            }
        }
    }

    Ok(options)
}

async fn run_volatility_surface_plot(symbol: &str) -> Result<()> {
    let config = Config::from_env()?;

    info!(
        "Starting real-time volatility surface monitor for {}",
        symbol
    );

    let rest_client = RestClient::new(config.alpaca.clone());
    let ws_client = WebSocketClient::new(config.alpaca.clone());

    info!("Fetching initial options chain for {}", symbol);
    let options_data = rest_client.get_options_chain(symbol, None).await?;
    let options = parse_options_chain(&options_data)?;
    info!("Found {} options for {}", options.len(), symbol);

    if options.is_empty() {
        warn!("No options found for {}. Exiting.", symbol);
        return Ok(());
    }

    let option_symbols: Vec<String> = options
        .iter()
        .map(|opt| opt.option_symbol.clone())
        .collect();

    let latest_quotes = Arc::new(RwLock::new(HashMap::new()));
    let surface = Arc::new(RwLock::new(None));
    let risk_free_rate = 0.03; // Could be made configurable

    let output_dir = Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir(output_dir)?;
    }

    info!(
        "Connecting to WebSocket for {} option symbols",
        option_symbols.len()
    );
    ws_client.connect(option_symbols).await?;

    let quotes_clone = latest_quotes.clone();
    let quote_task = tokio::spawn(async move {
        info!("Starting quote collection task");
        // Get notification channel to be notified when new data is available
        let mut notification_rx = ws_client.get_notification_channel();
        // Define batch size for processing quotes
        let batch_size = 50;

        loop {
            // Process quotes in batches for better efficiency
            match ws_client.next_option_quotes_batch(batch_size).await {
                Ok(batch) => {
                    if !batch.is_empty() {
                        let batch_len = batch.len();
                        let mut quotes = quotes_clone.write().await;
                        for quote in batch {
                            quotes.insert(quote.contract.option_symbol.clone(), quote);
                        }
                        debug!("Processed batch of {} quotes", batch_len);
                    }
                }
                Err(e) => {
                    warn!("Error getting quote batch: {}", e);
                }
            }

            // Wait for notification of new data instead of busy waiting with sleep
            match notification_rx.recv().await {
                Ok(_) => {
                    // New data is available, continue the loop to process it
                    continue;
                }
                Err(e) => {
                    // If the channel is closed, reconnect to it
                    warn!("Notification channel error: {}, reconnecting", e);
                    notification_rx = ws_client.get_notification_channel();
                    // Short sleep to avoid tight loop in case of persistent errors
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });

    let quotes_clone = latest_quotes.clone();
    let surface_clone = surface.clone();
    let update_interval = Duration::from_secs(5);
    let symbol_clone = symbol.to_string();

    let surface_task = tokio::spawn(async move {
        info!("Starting surface update task");
        loop {
            sleep(update_interval).await;

            let quotes = quotes_clone.read().await;
            if quotes.is_empty() {
                info!("No quotes available yet, waiting for data...");
                continue;
            }

            // Clone the quotes to avoid holding the lock during computation
            let quotes_vec: Vec<_> = quotes.values().cloned().collect();
            drop(quotes); // Release the lock before heavy computation

            // Offload compute-intensive tasks to a blocking task
            let ivs_result = tokio::task::spawn_blocking(move || {
                // Use rayon's parallel iterator for parallel processing
                use rayon::prelude::*;

                // Calculate implied volatilities in parallel
                let ivs: Vec<_> = quotes_vec
                    .into_par_iter()
                    .filter_map(|quote| {
                        match ImpliedVolatility::from_quote(&quote, risk_free_rate) {
                            Ok(iv) => Some(iv),
                            Err(e) => {
                                eprintln!(
                                    "Failed to calculate IV for {}: {}",
                                    quote.contract.option_symbol, e
                                );
                                None
                            }
                        }
                    })
                    .collect();
                ivs
            })
            .await;

            let ivs = match ivs_result {
                Ok(ivs) => ivs,
                Err(e) => {
                    warn!("Failed to calculate implied volatilities: {}", e);
                    continue;
                }
            };

            if ivs.is_empty() {
                warn!("No valid implied volatilities calculated");
                continue;
            }

            info!("Calculated {} implied volatilities", ivs.len());

            // Check if we already have a surface to update
            let mut should_create_new = false;
            {
                let surface_guard = surface_clone.read().await;
                should_create_new = surface_guard.is_none();
            }

            if should_create_new {
                // Create a new surface if we don't have one yet
                match VolatilitySurface::new(symbol_clone.clone(), &ivs) {
                    Ok(new_surface) => {
                        let mut surface_guard = surface_clone.write().await;
                        *surface_guard = Some(new_surface);
                        info!("Initial volatility surface created");
                    }
                    Err(e) => {
                        warn!("Failed to create volatility surface: {}", e);
                    }
                }
            } else {
                // Update existing surface
                let mut surface_guard = surface_clone.write().await;
                if let Some(ref mut existing_surface) = *surface_guard {
                    match existing_surface.update(&ivs) {
                        Ok(updated) => {
                            if updated {
                                info!("Volatility surface updated incrementally");
                            }
                        }
                        Err(e) => {
                            warn!("Failed to update volatility surface: {}", e);
                        }
                    }
                }
            }
        }
    });

    // spawn task to update visualizations
    let surface_clone = surface.clone();
    let plot_interval = Duration::from_millis(1000); // update every n milliseconds
    let max_update_interval = Duration::from_secs(10); // force update after this time even if no changes
    let output_dir_clone = output_dir.to_path_buf();
    let symbol_clone = symbol.to_string();

    let plot_task = tokio::spawn(async move {
        let mut last_update_time = std::time::Instant::now();
        let mut last_surface_version = 0;

        loop {
            sleep(plot_interval).await;
            let surface_guard = surface_clone.read().await;

            if let Some(ref surface) = *surface_guard {
                let current_version = surface.get_version();
                let time_since_update = last_update_time.elapsed();

                // Only update visualizations if the surface has changed or max interval has passed
                if current_version > last_surface_version || time_since_update > max_update_interval
                {
                    // update surface
                    let surface_path = output_dir_clone.join("volatility_surface.png");
                    if let Err(e) = plot_volatility_surface(surface, &surface_path) {
                        warn!("Failed to plot vol surface: {}", e);
                    }

                    // update smile for first expiration
                    if !surface.expirations.is_empty() {
                        let expiration = surface.expirations[0];
                        match surface.slice_by_expiration(expiration) {
                            Ok((strikes, vols)) => {
                                let smile_path = output_dir_clone.join("volatility_smile.png");
                                if let Err(e) = plot_volatility_smile(
                                    &strikes,
                                    &vols,
                                    &symbol_clone,
                                    &expiration,
                                    &smile_path,
                                ) {
                                    warn!("Failed to plot smile: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to slice volatility surface: {}", e);
                            }
                        }
                    }

                    info!("Visualizations updated (version: {})", current_version);
                    last_update_time = std::time::Instant::now();
                    last_surface_version = current_version;
                }
            } else {
                info!("No surface available yet for visualization");
            }
        }
    });

    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Shutting down...");
        }
        Err(err) => {
            warn!("Error waiting for Ctrl+C: {}", err);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    config.init_logging()?;

    let (ticker_sender, mut ticker_receiver) = mpsc::channel::<String>(10);

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let symbol = args[1].clone();
        info!("Ticker provided as command-line argument: {}", symbol);
        run_volatility_surface_plot(&symbol).await?;
        return Ok(());
    }

    info!("Starting GUI for ticker input");
    let plotting_task = tokio::spawn(async move {
        while let Some(ticker) = ticker_receiver.recv().await {
            info!("Received ticker from GUI: {}", ticker);
            if let Err(e) = run_volatility_surface_plot(&ticker).await {
                warn!("Error plotting volatility surface for {}: {}", ticker, e);
            }
        }
    });

    let app = VolatilitySurfaceApp {
        ticker_input: String::new(),
        status: "Enter a ticker symbol and click 'Plot Volatility Surface'".to_string(),
        ticker_sender,
    };

    let native_options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(400.0, 200.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Live Volatility Surface Plotter",
        native_options,
        Box::new(|_cc| Box::new(app)),
    )
    .map_err(|e| {
        let err_msg = format!("Failed to start GUI: {}", e);
        warn!("{}", err_msg);
        OptionsError::Other(err_msg)
    })?;

    info!("shutting down");
    Ok(())
}
