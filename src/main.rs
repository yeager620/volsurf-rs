//! Example application for options-rs
//!
//! This example demonstrates how to use the options-rs library to:
//! 1. Connect to Alpaca Markets API
//! 2. Stream real-time options data
//! 3. Calculate implied volatility
//! 4. Generate and visualize a volatility surface

use options_rs::api::{RestClient, WebSocketClient};
use options_rs::config::Config;
use options_rs::error::Result;
use options_rs::models::volatility::{ImpliedVolatility, VolatilitySurface};
use options_rs::utils::{plot_volatility_smile, plot_volatility_surface};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Step 1: Load configuration from environment variables
    let config = Config::from_env()?;
    config.init_logging()?;

    info!("Starting options-rs example application");

    // Step 2: Create API clients
    let rest_client = RestClient::new(config.alpaca.clone());
    let ws_client = WebSocketClient::new(config.alpaca.clone());

    // Step 3: Get account information
    let account = rest_client.get_account().await?;
    info!("Account: {} (${:.2})", account.id, account.equity);

    // Step 4: Choose a symbol and get available options
    let symbol = "AAPL"; // Example: Apple Inc.
    info!("Getting options for {}", symbol);

    let options = rest_client.get_options(symbol).await?;
    info!("Found {} options for {}", options.len(), symbol);

    if options.is_empty() {
        warn!("No options found for {}. Exiting.", symbol);
        return Ok(());
    }

    // Step 5: Extract option symbols for WebSocket subscription
    let option_symbols: Vec<String> = options.iter()
        .map(|opt| opt.symbol.clone())
        .take(50) // Limit to 50 options for this example
        .collect();

    info!("Connecting to WebSocket for {} option symbols", option_symbols.len());

    // Step 6: Connect to WebSocket and subscribe to option data
    ws_client.connect(option_symbols).await?;

    // Step 7: Process option quotes and calculate implied volatility
    info!("Processing option quotes and calculating implied volatility");

    let mut ivs = Vec::new();
    let risk_free_rate = 0.03; // 3% risk-free rate
    let max_quotes = 100; // Process up to 100 quotes
    let mut quotes_processed = 0;

    // Create a map to store the latest quote for each option
    let mut latest_quotes = HashMap::new();

    // Process quotes until we have enough data or timeout
    let timeout = tokio::time::Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    while quotes_processed < max_quotes && start_time.elapsed() < timeout {
        // Get the next quote with a timeout
        if let Ok(Some(quote)) = tokio::time::timeout(
            tokio::time::Duration::from_secs(1),
            ws_client.next_option_quote(),
        ).await? {
            // Store the latest quote for each option
            latest_quotes.insert(quote.contract.option_symbol.clone(), quote.clone());
            quotes_processed += 1;

            if quotes_processed % 10 == 0 {
                info!("Processed {} quotes", quotes_processed);
            }
        }
    }

    info!("Received {} unique option quotes", latest_quotes.len());

    // Calculate implied volatility for each quote
    for (_, quote) in latest_quotes {
        match ImpliedVolatility::from_quote(&quote, risk_free_rate) {
            Ok(iv) => {
                info!(
                    "Option: {}, Strike: {}, Expiry: {}, IV: {:.2}%",
                    quote.contract.option_symbol,
                    quote.contract.strike,
                    quote.contract.expiration.format("%Y-%m-%d"),
                    iv.value * 100.0
                );
                ivs.push(iv);
            }
            Err(e) => {
                warn!("Failed to calculate IV for {}: {}", quote.contract.option_symbol, e);
            }
        }
    }

    if ivs.is_empty() {
        warn!("No valid implied volatility calculations. Exiting.");
        return Ok(());
    }

    info!("Successfully calculated {} implied volatilities", ivs.len());

    // Step 8: Create volatility surface
    info!("Creating volatility surface");

    let surface = VolatilitySurface::new(symbol.to_string(), &ivs)?;

    // Step 9: Plot volatility surface and smiles
    info!("Plotting volatility surface");

    let output_dir = Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir(output_dir)?;
    }

    // Plot the full volatility surface
    let surface_path = output_dir.join("volatility_surface.png");
    plot_volatility_surface(&surface, &surface_path)?;
    info!("Volatility surface saved to {:?}", surface_path);

    // Plot volatility smile for the first expiration
    if !surface.expirations.is_empty() {
        let expiration = surface.expirations[0];
        let (strikes, vols) = surface.slice_by_expiration(expiration)?;

        let smile_path = output_dir.join("volatility_smile.png");
        plot_volatility_smile(&strikes, &vols, symbol, &expiration, &smile_path)?;
        info!("Volatility smile saved to {:?}", smile_path);
    }

    info!("Example completed successfully");

    Ok(())
}

