use options_rs::api::{RestClient, WebSocketClient};
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::volatility::{ImpliedVolatility, VolatilitySurface};
use options_rs::models::{OptionContract, OptionQuote};
use options_rs::utils::{plot_volatility_smile, plot_volatility_surface};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

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

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration and initialize logging
    let config = Config::from_env()?;
    config.init_logging()?;

    // Get ticker symbol from command line or use default
    let args: Vec<String> = std::env::args().collect();
    let symbol = args.get(1).map(|s| s.as_str()).unwrap_or("AAPL");

    info!(
        "Starting real-time volatility surface monitor for {}",
        symbol
    );

    // Initialize API clients
    let rest_client = RestClient::new(config.alpaca.clone());
    let ws_client = WebSocketClient::new(config.alpaca.clone());

    // Fetch initial options chain
    info!("Fetching initial options chain for {}", symbol);
    let options_data = rest_client.get_options_chain(symbol, None).await?;
    let options = parse_options_chain(&options_data)?;
    info!("Found {} options for {}", options.len(), symbol);

    if options.is_empty() {
        warn!("No options found for {}. Exiting.", symbol);
        return Ok(());
    }

    // Extract option symbols for WebSocket subscription
    let option_symbols: Vec<String> = options
        .iter()
        .map(|opt| opt.option_symbol.clone())
        .collect();

    // Create shared state
    let latest_quotes = Arc::new(RwLock::new(HashMap::new()));
    let surface = Arc::new(RwLock::new(None));
    let risk_free_rate = 0.03; // Could be made configurable

    // Set up output directory
    let output_dir = Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir(output_dir)?;
    }

    // Connect to WebSocket
    info!(
        "Connecting to WebSocket for {} option symbols",
        option_symbols.len()
    );
    ws_client.connect(option_symbols).await?;

    // Spawn task to collect quotes
    let quotes_clone = latest_quotes.clone();
    let quote_task = tokio::spawn(async move {
        info!("Starting quote collection task");
        loop {
            if let Ok(Some(quote)) = ws_client.next_option_quote().await {
                let mut quotes = quotes_clone.write().await;
                quotes.insert(quote.contract.option_symbol.clone(), quote);
            }
            // Small sleep to prevent CPU spinning
            sleep(Duration::from_millis(1)).await;
        }
    });

    // Spawn task to update surface
    let quotes_clone = latest_quotes.clone();
    let surface_clone = surface.clone();
    let update_interval = Duration::from_secs(5); // Update every 5 seconds
    let symbol_clone = symbol.to_string(); // Clone the symbol for the surface update task
    let surface_task = tokio::spawn(async move {
        info!("Starting surface update task");
        loop {
            sleep(update_interval).await;

            // Calculate implied volatilities from latest quotes
            let quotes = quotes_clone.read().await;
            if quotes.is_empty() {
                info!("No quotes available yet, waiting for data...");
                continue;
            }

            let mut ivs = Vec::new();
            for (_, quote) in quotes.iter() {
                match ImpliedVolatility::from_quote(quote, risk_free_rate) {
                    Ok(iv) => {
                        ivs.push(iv);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to calculate IV for {}: {}",
                            quote.contract.option_symbol, e
                        );
                    }
                }
            }

            if ivs.is_empty() {
                warn!("No valid implied volatilities calculated");
                continue;
            }

            info!("Calculated {} implied volatilities", ivs.len());

            // Create new volatility surface
            match VolatilitySurface::new(symbol_clone.clone(), &ivs) {
                Ok(new_surface) => {
                    let mut surface_guard = surface_clone.write().await;
                    *surface_guard = Some(new_surface);
                    info!("Volatility surface updated");
                }
                Err(e) => {
                    warn!("Failed to create volatility surface: {}", e);
                }
            }
        }
    });

    // spawn task to update visualizations
    let surface_clone = surface.clone();
    let plot_interval = Duration::from_millis(1000); // Update plots every x milliseconds
    let output_dir_clone = output_dir.to_path_buf();
    let symbol_clone = symbol.to_string();

    let plot_task = tokio::spawn(async move {
        loop {
            sleep(plot_interval).await;
            let surface_guard = surface_clone.read().await;
            if let Some(ref surface) = *surface_guard {
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

                info!("Visualizations updated");
            } else {
                info!("No surface available yet for visualization");
            }
        }
    });

    info!("Press Ctrl+C to exit");
    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Shutting down...");
        }
        Err(err) => {
            warn!("Error waiting for Ctrl+C: {}", err);
        }
    }

    // cleanup here?

    Ok(())
}
