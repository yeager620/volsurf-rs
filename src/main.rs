//! Example application for options-rs
//!
//! This example demonstrates how to use the options-rs library to:
//! 1. Connect to Alpaca Markets API
//! 2. Stream real-time options data
//! 3. Calculate implied volatility
//! 4. Generate and visualize a volatility surface

use options_rs::api::{RestClient, WebSocketClient};
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::{OptionContract, OptionQuote};
use options_rs::models::volatility::{ImpliedVolatility, VolatilitySurface};
use options_rs::utils::{plot_volatility_smile, plot_volatility_surface};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};
use serde_json::Value;
use tokio::time::error::Elapsed;

/// Parse options chain data from Alpaca API response
fn parse_options_chain(data: &Value) -> Result<Vec<OptionContract>> {
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

                        let contract = OptionContract::new(
                            symbol.to_string(),
                            option_type,
                            strike,
                            exp_utc,
                        );

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
    let config = Config::from_env()?;
    config.init_logging()?;

    info!("Starting options-rs example application");

    let rest_client = RestClient::new(config.alpaca.clone());
    let ws_client = WebSocketClient::new(config.alpaca.clone());

    let account = rest_client.get_account().await?;
    info!("Account: {} (${:.2})", account.id, account.equity);

    let symbol = "AAPL"; // Example: Apple Inc.
    info!("Getting options for {}", symbol);

    let options_data = rest_client.get_options_chain(symbol, None).await?;

    let options = parse_options_chain(&options_data)?;
    info!("Found {} options for {}", options.len(), symbol);

    if options.is_empty() {
        warn!("No options found for {}. Exiting.", symbol);
        return Ok(());
    }

    let option_symbols: Vec<String> = options.iter()
        .map(|opt| opt.symbol.clone())
        .take(50) // Limit to 50 options for this example
        .collect();

    info!("Connecting to WebSocket for {} option symbols", option_symbols.len());

    ws_client.connect(option_symbols).await?;

    info!("Processing option quotes and calculating implied volatility");

    let mut ivs = Vec::new();
    let risk_free_rate = 0.03; // 3% risk-free rate
    let max_quotes = 100; // Process up to 100 quotes
    let mut quotes_processed = 0;

    let mut latest_quotes = HashMap::new();

    let timeout = tokio::time::Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    while quotes_processed < max_quotes && start_time.elapsed() < timeout {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(1),
            ws_client.next_option_quote(),
        ).await {
            Ok(Ok(Some(quote))) => {
                latest_quotes.insert(quote.contract.option_symbol.clone(), quote.clone());
                quotes_processed += 1;

                if quotes_processed % 10 == 0 {
                    info!("Processed {} quotes", quotes_processed);
                }
            },
            Ok(Ok(None)) => {},
            Ok(Err(e)) => {
                return Err(e);
            },
            Err(_) => {}
        }
    }

    info!("Received {} unique option quotes", latest_quotes.len());

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

    info!("Creating volatility surface");

    let surface = VolatilitySurface::new(symbol.to_string(), &ivs)?;

    info!("Plotting volatility surface");

    let output_dir = Path::new("output");
    if !output_dir.exists() {
        std::fs::create_dir(output_dir)?;
    }

    let surface_path = output_dir.join("volatility_surface.png");
    plot_volatility_surface(&surface, &surface_path)?;
    info!("Volatility surface saved to {:?}", surface_path);

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
