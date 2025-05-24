use eframe::egui;
use image;
use options_rs::api::{RestClient, WebSocketClient};
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::volatility::{ImpliedVolatility, VolatilitySurface};
use options_rs::models::OptionContract;
use options_rs::utils::{
    plot_volatility_smile, plot_volatility_smile_in_memory, plot_volatility_surface,
    plot_volatility_surface_in_memory,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

struct PlotImages {
    surface: Option<egui::TextureHandle>,
    smile: Option<egui::TextureHandle>,
}

struct PlotData {
    surface_png: Vec<u8>,
    smile_png: Option<Vec<u8>>,
}

struct VolatilitySurfaceApp {
    ticker_input: String,
    status: String,
    ticker_sender: mpsc::Sender<String>,
    plot_receiver: mpsc::Receiver<PlotData>,
    plots: PlotImages,
}

impl eframe::App for VolatilitySurfaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(plot_data) = self.plot_receiver.try_recv() {
            self.status = "Received new plot data".to_string();
            if !plot_data.surface_png.is_empty() {
                let surface_texture =
                    load_texture_from_png(ctx, &plot_data.surface_png, "surface_texture");
                self.plots.surface = Some(surface_texture);
            }
            if let Some(smile_png) = plot_data.smile_png {
                if !smile_png.is_empty() {
                    let smile_texture = load_texture_from_png(ctx, &smile_png, "smile_texture");
                    self.plots.smile = Some(smile_texture);
                }
            }
        }

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

                        if let Err(e) = self.ticker_sender.try_send(ticker) {
                            self.status = format!("Error: {}", e);
                        }
                    }
                }
            });

            ui.separator();
            ui.label(&self.status);
            ui.separator();

            if self.plots.surface.is_some() || self.plots.smile.is_some() {
                _frame.set_window_size(egui::vec2(1000.0, 700.0));

                ui.columns(2, |columns| {
                    if let Some(ref surface) = self.plots.surface {
                        columns[0].heading("Volatility Surface");
                        columns[0].image(surface, egui::vec2(480.0, 360.0));
                    } else {
                        columns[0].label("Volatility Surface: Waiting for data...");
                    }
                    if let Some(ref smile) = self.plots.smile {
                        columns[1].heading("Volatility Smile");
                        columns[1].image(smile, egui::vec2(480.0, 360.0));
                    } else {
                        columns[1].label("Volatility Smile: Waiting for data...");
                    }
                });
            } else {
                ui.label("Waiting for plot data...");
                ui.label("Press Ctrl+C in the terminal to exit the application.");
            }
        });
    }
}

fn load_texture_from_png(
    ctx: &egui::Context,
    png_data: &[u8],
    texture_id: &str,
) -> egui::TextureHandle {
    let image = image::load_from_memory(png_data)
        .expect("Failed to load PNG data")
        .to_rgba8();
    let size = [image.width() as _, image.height() as _];
    let image_data =
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_flat_samples().as_slice());

    ctx.load_texture(texture_id, image_data, egui::TextureOptions::default())
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

async fn run_volatility_surface_plot(
    symbol: &str,
    plot_sender: mpsc::Sender<PlotData>,
) -> Result<()> {
    let config = Config::from_env()?;

    info!(
        "Starting real-time volatility surface monitor for {}",
        symbol
    );

    let rest_client = RestClient::new(config.alpaca.clone());
    let ws_client = WebSocketClient::new(config.alpaca.clone());

    info!("Fetching initial options chain for {}", symbol);
    let options_data = rest_client
        .get_options_chain(
            symbol, None, // expiration_date
            None, // option_type
            None, // strike_lower
            None, // strike_upper
            None, // limit_per_expiration
            None, // limit_strikes
            None, // greeks
        )
        .await?;
    let options = options_data.results;
    info!("Found {} options for {}", options.len(), symbol);

    if options.is_empty() {
        warn!("No options found for {}. Exiting.", symbol);
        return Ok(());
    }

    let option_symbols: Vec<String> = options.iter().map(|opt| opt.symbol.clone()).collect();

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
        let mut notification_rx = ws_client.get_notification_channel();
        let batch_size = 50;

        loop {
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

            match notification_rx.recv().await {
                Ok(_) => {
                    continue;
                }
                Err(e) => {
                    warn!("Notification channel error: {}, reconnecting", e);
                    notification_rx = ws_client.get_notification_channel();
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

            let quotes_vec: Vec<_> = quotes.values().cloned().collect();
            drop(quotes); 

            let ivs_result = tokio::task::spawn_blocking(move || {
                use rayon::prelude::*;

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

            let mut should_create_new = false;
            {
                let surface_guard = surface_clone.read().await;
                should_create_new = surface_guard.is_none();
            }

            if should_create_new {
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
    let plot_sender_clone = plot_sender.clone();

    let plot_task = tokio::spawn(async move {
        let mut last_update_time = std::time::Instant::now();
        let mut last_surface_version = 0;

        loop {
            sleep(plot_interval).await;
            let surface_guard = surface_clone.read().await;

            if let Some(ref surface) = *surface_guard {
                let current_version = surface.get_version();
                let time_since_update = last_update_time.elapsed();

                if current_version > last_surface_version || time_since_update > max_update_interval
                {
                    let mut plot_data = PlotData {
                        surface_png: Vec::new(),
                        smile_png: None,
                    };

                    match plot_volatility_surface_in_memory(surface) {
                        Ok(surface_png) => {
                            let surface_path = output_dir_clone.join("volatility_surface.png");
                            if let Err(e) = std::fs::write(&surface_path, &surface_png) {
                                warn!("Failed to save vol surface to file: {}", e);
                            }

                            plot_data.surface_png = surface_png;
                        }
                        Err(e) => {
                            warn!("Failed to generate vol surface plot: {}", e);
                            continue;
                        }
                    }

                    if !surface.expirations.is_empty() {
                        let expiration = surface.expirations[0];
                        match surface.slice_by_expiration(expiration) {
                            Ok((strikes, vols)) => {
                                match plot_volatility_smile_in_memory(
                                    &strikes,
                                    &vols,
                                    &symbol_clone,
                                    &expiration,
                                ) {
                                    Ok(smile_png) => {
                                        let smile_path =
                                            output_dir_clone.join("volatility_smile.png");
                                        if let Err(e) = std::fs::write(&smile_path, &smile_png) {
                                            warn!("Failed to save vol smile to file: {}", e);
                                        }

                                        plot_data.smile_png = Some(smile_png);
                                    }
                                    Err(e) => {
                                        warn!("Failed to generate vol smile plot: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to slice volatility surface: {}", e);
                            }
                        }
                    }

                    if let Err(e) = plot_sender_clone.try_send(plot_data) {
                        warn!("Failed to send plot data to GUI: {}", e);
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
    let (plot_sender, plot_receiver) = mpsc::channel::<PlotData>(10);

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let symbol = args[1].clone();
        info!("Ticker provided as command-line argument: {}", symbol);
        run_volatility_surface_plot(&symbol, plot_sender.clone()).await?;
        return Ok(());
    }

    info!("Starting GUI for ticker input");
    let plotting_task = tokio::spawn(async move {
        while let Some(ticker) = ticker_receiver.recv().await {
            info!("Received ticker from GUI: {}", ticker);
            if let Err(e) = run_volatility_surface_plot(&ticker, plot_sender.clone()).await {
                warn!("Error plotting volatility surface for {}: {}", ticker, e);
            }
        }
    });

    let app = VolatilitySurfaceApp {
        ticker_input: String::new(),
        status: "Enter a ticker symbol and click 'Plot Volatility Surface'".to_string(),
        ticker_sender,
        plot_receiver,
        plots: PlotImages {
            surface: None,
            smile: None,
        },
    };

    let native_options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(1000.0, 700.0)),
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
