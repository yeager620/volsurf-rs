use eframe::egui;
use image;
use options_rs::api::{RestClient, WebSocketClient};
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::volatility::{ImpliedVolatility, VolatilitySurface};
use options_rs::models::{OptionContract, OptionQuote, OptionType};
use options_rs::utils::{
    plot_volatility_smile, plot_volatility_smile_in_memory, plot_volatility_surface,
    plot_volatility_surface_in_memory,
};
use chrono::{DateTime, Utc};
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
    ticker_sender: mpsc::Sender<(String, DataSource)>,
    plot_receiver: mpsc::Receiver<PlotData>,
    plots: PlotImages,
    data_source: DataSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DataSource {
    LiveUpdates,
    MostRecentOptionsChain,
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
            });

            ui.horizontal(|ui| {
                ui.label("Data Source:");
                ui.radio_value(
                    &mut self.data_source,
                    DataSource::LiveUpdates,
                    "Live Updates (placeholder)",
                );
                ui.radio_value(
                    &mut self.data_source,
                    DataSource::MostRecentOptionsChain,
                    "Most Recent Options Chain",
                );
            });

            ui.horizontal(|ui| {
                if ui.button("Plot Volatility Surface").clicked() {
                    if self.ticker_input.trim().is_empty() {
                        self.status = "Please enter a ticker symbol".to_string();
                    } else {
                        let ticker = self.ticker_input.trim().to_uppercase();
                        let data_source_str = match self.data_source {
                            DataSource::LiveUpdates => "live updates",
                            DataSource::MostRecentOptionsChain => "most recent options chain",
                        };
                        self.status = format!(
                            "Starting volatility surface plotting for {} using {}",
                            ticker, data_source_str
                        );

                        if let Err(e) = self.ticker_sender.try_send((ticker, self.data_source)) {
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
    data_source: DataSource,
) -> Result<()> {
    let config = Config::from_env()?;

    match data_source {
        DataSource::LiveUpdates => {
            info!(
                "Starting real-time volatility surface monitor for {} (placeholder)",
                symbol
            );
        }
        DataSource::MostRecentOptionsChain => {
            info!(
                "Starting most recent options chain volatility surface for {}",
                symbol
            );
        }
    }

    let rest_client = Arc::new(RestClient::new(config.alpaca.clone()));

    // Get option contracts for the symbol
    info!("Fetching options chain for {}", symbol);
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
    info!(
        "Found {} options for {} using get_options_chain API",
        options.len(),
        symbol
    );

    // Only exit early if we're using LiveUpdates data source and no options are found
    if options.is_empty() && data_source == DataSource::LiveUpdates {
        warn!("No options found for {} using get_options_chain API. This symbol may not have options available or there might be an issue with the API. Exiting.", symbol);
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

    // Different data fetching strategies based on the data source
    match data_source {
        DataSource::LiveUpdates => {
            // This is a placeholder for live updates
            // For now, we'll use the existing WebSocket implementation
            let ws_client = WebSocketClient::new(config.alpaca.clone());

            info!(
                "Connecting to WebSocket for {} option symbols",
                option_symbols.len()
            );
            ws_client.connect(option_symbols.clone()).await?;

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
        }
        DataSource::MostRecentOptionsChain => {
            // Fetch the most recent options chain using the snapshots API
            info!(
                "Fetching most recent options chain snapshots for {}",
                symbol
            );

            // Get option chain snapshots
            info!(
                "Calling get_option_chain_snapshots API for {} with feed=indicative and limit=100",
                symbol
            );
            let snapshots = rest_client
                .get_option_chain_snapshots(
                    symbol,
                    Some("indicative"), // Use indicative feed as mentioned in the docs
                    Some(100),          // Limit to 100 snapshots
                    None,               // updated_since
                    None,               // page_token
                    None,               // option_type
                    None,               // strike_price_gte
                    None,               // strike_price_lte
                    None,               // expiration_date
                    None,               // expiration_date_gte
                    None,               // expiration_date_lte
                    None,               // root_symbol
                )
                .await?;

            let snapshot_count = snapshots.snapshots.len();
            info!("Fetched {} option snapshots for {}", snapshot_count, symbol);

            if snapshot_count == 0 {
                warn!("No option snapshots found for {} using get_option_chain_snapshots API with feed=indicative.", symbol);
                warn!("This could be because:");
                warn!(
                    "1. The symbol {} does not have any options available",
                    symbol
                );
                warn!(
                    "2. The symbol {} is not valid or not supported by Alpaca",
                    symbol
                );
                warn!(
                    "3. There might be an issue with your Alpaca API credentials or subscription"
                );
                warn!("4. The Alpaca API might be experiencing issues");
                warn!("Please check the symbol and try again, or try a different symbol.");
            }

            // Log the first few snapshots for debugging
            if snapshot_count > 0 {
                let sample_count = std::cmp::min(5, snapshot_count);
                info!("Sample of {} snapshots for debugging:", sample_count);
                for (i, (symbol, snapshot)) in snapshots.snapshots.iter().take(sample_count).enumerate() {
                    info!("Snapshot {}: Symbol={}", i+1, symbol);

                    // Log the raw snapshot data to see its structure
                    info!("  Raw snapshot data: {:?}", snapshot);

                    if let Some(last_quote) = &snapshot.last_quote {
                        info!("  Last Quote: bid={}, ask={}, timestamp={}", last_quote.bid, last_quote.ask, last_quote.t);
                    } else {
                        info!("  No Last Quote available");
                    }
                    if let Some(last_trade) = &snapshot.last_trade {
                        info!("  Last Trade: price={}, size={}, timestamp={}", last_trade.price, last_trade.size, last_trade.t);
                    } else {
                        info!("  No Last Trade available");
                    }
                }
            }

            // Convert snapshots to option quotes
            let mut quotes = latest_quotes.write().await;
            let mut parse_failures = 0;
            let mut missing_data_count = 0;
            let mut fallback_successes = 0;
            let mut manual_contract_creations = 0;

            for (symbol_key, snapshot) in snapshots.snapshots.iter() {
                info!("Processing snapshot for symbol key: {}", symbol_key);

                // Try to create a contract from the OCC symbol first
                let contract_result = OptionContract::from_occ_symbol(symbol_key);

                // If that fails, try to create a contract manually from the OCC symbol
                let contract = match contract_result {
                    Some(c) => {
                        info!("Successfully parsed OCC symbol: {} -> {}", symbol_key, c.option_symbol);
                        Some(c)
                    },
                    None => {
                        parse_failures += 1;
                        warn!("Failed to parse OCC symbol: {}", symbol_key);

                        // Try to manually parse the OCC symbol
                        // Format: AAPL250530C00145000
                        let c_pos = symbol_key.find('C');
                        let p_pos = symbol_key.find('P');

                        if c_pos.is_some() || p_pos.is_some() {
                            let type_pos = if c_pos.is_some() { c_pos.unwrap() } else { p_pos.unwrap() };

                            if type_pos >= 6 && type_pos + 1 < symbol_key.len() {
                                let underlying = &symbol_key[0..(type_pos - 6)];
                                let date_str = &symbol_key[(type_pos - 6)..type_pos];
                                let option_type_char = symbol_key.chars().nth(type_pos).unwrap();
                                let strike_str = &symbol_key[(type_pos + 1)..];

                                info!("Manual parsing: underlying={}, date={}, type={}, strike={}", 
                                      underlying, date_str, option_type_char, strike_str);

                                // Try to parse the date
                                if date_str.len() == 6 {
                                    if let (Ok(year), Ok(month), Ok(day)) = (
                                        date_str[0..2].parse::<i32>(),
                                        date_str[2..4].parse::<u32>(),
                                        date_str[4..6].parse::<u32>()
                                    ) {
                                        // Try to parse the strike price
                                        if let Ok(strike_int) = strike_str.parse::<u32>() {
                                            let strike = strike_int as f64 / 1000.0;

                                            // Create the expiration date
                                            if let Some(naive_date) = chrono::NaiveDate::from_ymd_opt(2000 + year, month, day) {
                                                if let Some(naive_datetime) = naive_date.and_hms_opt(16, 0, 0) {
                                                    if let Some(expiration) = naive_datetime.and_local_timezone(chrono::Utc).single() {
                                                        let option_type = if option_type_char == 'C' {
                                                            OptionType::Call
                                                        } else {
                                                            OptionType::Put
                                                        };

                                                        let contract = OptionContract::new(
                                                            underlying.to_string(),
                                                            option_type,
                                                            strike,
                                                            expiration
                                                        );

                                                        manual_contract_creations += 1;
                                                        info!("Manually created contract: {}", contract.option_symbol);
                                                        Some(contract)
                                                    } else {
                                                        warn!("Failed to convert datetime to UTC");
                                                        None
                                                    }
                                                } else {
                                                    warn!("Failed to create datetime");
                                                    None
                                                }
                                            } else {
                                                warn!("Failed to create date from {}-{}-{}", 2000 + year, month, day);
                                                None
                                            }
                                        } else {
                                            warn!("Failed to parse strike price: {}", strike_str);
                                            None
                                        }
                                    } else {
                                        warn!("Failed to parse date components from: {}", date_str);
                                        None
                                    }
                                } else {
                                    warn!("Date string not 6 characters: {}", date_str);
                                    None
                                }
                            } else {
                                warn!("Invalid type position: {}", type_pos);
                                None
                            }
                        } else {
                            warn!("No 'C' or 'P' found in symbol: {}", symbol_key);
                            None
                        }
                    }
                };

                // If we have a valid contract, try to create an option quote
                if let Some(contract) = contract {
                    // Extract quote data from various sources
                    let mut bid: Option<f64> = None;
                    let mut ask: Option<f64> = None;
                    let mut last_price: Option<f64> = None;
                    let mut volume: Option<u64> = None;
                    let mut timestamp: Option<DateTime<Utc>> = None;

                    // Try to get data from last_quote and last_trade first
                    if let Some(quote) = &snapshot.last_quote {
                        bid = Some(quote.bid);
                        ask = Some(quote.ask);
                        timestamp = Some(quote.t);
                    }

                    if let Some(trade) = &snapshot.last_trade {
                        last_price = Some(trade.price);
                        volume = Some(trade.size);
                        if timestamp.is_none() {
                            timestamp = Some(trade.t);
                        }
                    }

                    // If we don't have bid/ask from last_quote, try to get from dailyBar or minuteBar
                    if bid.is_none() || ask.is_none() {
                        if let Some(bar) = &snapshot.dailyBar {
                            // Use close as both bid and ask if we don't have them
                            if bid.is_none() {
                                bid = Some(bar.c * 0.99); // Slightly lower than close for bid
                            }
                            if ask.is_none() {
                                ask = Some(bar.c * 1.01); // Slightly higher than close for ask
                            }
                            if timestamp.is_none() {
                                timestamp = Some(bar.t);
                            }
                        } else if let Some(bar) = &snapshot.minuteBar {
                            // Use close as both bid and ask if we don't have them
                            if bid.is_none() {
                                bid = Some(bar.c * 0.99); // Slightly lower than close for bid
                            }
                            if ask.is_none() {
                                ask = Some(bar.c * 1.01); // Slightly higher than close for ask
                            }
                            if timestamp.is_none() {
                                timestamp = Some(bar.t);
                            }
                        }
                    }

                    // If we don't have last_price, try to get from dailyBar or minuteBar
                    if last_price.is_none() {
                        if let Some(bar) = &snapshot.dailyBar {
                            last_price = Some(bar.c); // Use close as last price
                            if volume.is_none() {
                                volume = Some(bar.v);
                            }
                        } else if let Some(bar) = &snapshot.minuteBar {
                            last_price = Some(bar.c); // Use close as last price
                            if volume.is_none() {
                                volume = Some(bar.v);
                            }
                        } else if let Some(bar) = &snapshot.prevDailyBar {
                            last_price = Some(bar.c); // Use close as last price
                            if volume.is_none() {
                                volume = Some(bar.v);
                            }
                        }
                    }

                    // If we still don't have a timestamp, use current time
                    if timestamp.is_none() {
                        timestamp = Some(Utc::now());
                    }

                    debug!("Extracted data for {}: bid={:?}, ask={:?}, last_price={:?}, volume={:?}, timestamp={:?}",
                           symbol, bid, ask, last_price, volume, timestamp);

                    if bid.is_some() && ask.is_some() && last_price.is_some() && timestamp.is_some() {
                        let bid = bid.unwrap();
                        let ask = ask.unwrap();
                        let last_price = last_price.unwrap();
                        let volume = volume.unwrap_or(0);
                        let timestamp = timestamp.unwrap();

                        // Estimate underlying price (not ideal but workable)
                        // In a real implementation, you might want to fetch the underlying price separately
                        let underlying_price = if contract.is_call() {
                            contract.strike + ask - bid
                        } else {
                            contract.strike - ask + bid
                        };

                        let quote = OptionQuote {
                            contract,
                            bid,
                            ask,
                            last: last_price,
                            volume,
                            open_interest: 0, // Not available in snapshots
                            underlying_price,
                            timestamp,
                        };

                        quotes.insert(symbol_key.to_string(), quote);
                    } else {
                        missing_data_count += 1;
                        debug!(
                            "Skipping option {} because it's missing required price data",
                            symbol_key
                        );
                    }
                }
            }


            info!("OCC symbol parse failures: {}/{}", parse_failures, snapshot_count);
            info!("Fallback contract creations: {}/{}", fallback_successes, parse_failures);
            info!("Missing quote/trade data: {}/{}", missing_data_count, snapshot_count);

            let quote_count = quotes.len();
            info!("Processed {} option quotes from snapshots", quote_count);

            if quote_count == 0 {
                warn!(
                    "No valid option quotes could be created from snapshots for {}.",
                    symbol
                );
                warn!("This could be because:");
                warn!("1. The snapshots don't contain the necessary quote and trade data");
                warn!("2. There might be an issue with parsing the OCC symbols");
                warn!("3. The Alpaca API might be returning incomplete data");
                warn!("Please try a different symbol or check your API subscription.");
                return Ok(());
            }
        }
    }

    let quotes_clone = latest_quotes.clone();
    let surface_clone = surface.clone();
    let update_interval = Duration::from_secs(5);
    let symbol_clone = symbol.to_string();
    let rest_client_clone = Arc::clone(&rest_client);
    let option_symbols_clone = option_symbols.clone();

    let surface_task = tokio::spawn(async move {
        info!("Starting surface update task");
        loop {
            sleep(update_interval).await;

            let quotes = quotes_clone.read().await;
            let quotes_vec: Vec<_>;

            if quotes.is_empty() {
                info!("No quotes available, fetching recent quotes from REST API...");
                drop(quotes);

                // Convert Vec<String> to Vec<&str> for the API call
                let option_symbols_str: Vec<&str> =
                    option_symbols_clone.iter().map(AsRef::as_ref).collect();

                // Fetch latest quotes using the REST API
                match rest_client_clone
                    .get_options_quotes(&option_symbols_str)
                    .await
                {
                    Ok(response) => {
                        let quotes_response = response.quotes;
                        if quotes_response.is_empty() {
                            info!("No option quotes available, waiting for data...");
                            continue;
                        }

                        info!("Fetched {} option quotes", quotes_response.len());

                        // Convert API quotes to OptionQuote objects
                        quotes_vec = quotes_response
                            .iter()
                            .filter_map(|(symbol, quote)| {
                                if let Some(contract) = OptionContract::from_occ_symbol(symbol) {
                                    // Estimate underlying price (not ideal but workable)
                                    // In a real implementation, you might want to fetch the underlying price separately
                                    let underlying_price = if contract.is_call() {
                                        contract.strike + quote.ask - quote.bid
                                    } else {
                                        contract.strike - quote.ask + quote.bid
                                    };

                                    Some(OptionQuote {
                                        contract,
                                        bid: quote.bid,
                                        ask: quote.ask,
                                        last: (quote.bid + quote.ask) / 2.0, // Use mid as last
                                        volume: quote.size_bid + quote.size_ask,
                                        open_interest: 0, // Not available in quotes
                                        underlying_price,
                                        timestamp: quote.t,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if quotes_vec.is_empty() {
                            info!("No valid option quotes could be created, waiting for data...");
                            continue;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to fetch option quotes: {}", e);
                        continue;
                    }
                }
            } else {
                quotes_vec = quotes.values().cloned().collect();
                drop(quotes);
            }

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
                warn!(
                    "No valid implied volatilities calculated for {}",
                    symbol_clone
                );
                warn!("This could be because:");
                warn!("1. The option quotes don't have valid bid/ask prices");
                warn!("2. The implied volatility calculation failed due to numerical issues");
                warn!("3. The options might be too far from the money or too close to expiration");
                warn!("Please try a different symbol or check the option data quality.");
                continue;
            }

            info!(
                "Calculated {} implied volatilities for {}",
                ivs.len(),
                symbol_clone
            );

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

    let (ticker_sender, mut ticker_receiver) = mpsc::channel::<(String, DataSource)>(10);
    let (plot_sender, plot_receiver) = mpsc::channel::<PlotData>(10);

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let symbol = args[1].clone();
        info!("Ticker provided as command-line argument: {}", symbol);
        // Default to most recent options chain for command-line usage
        run_volatility_surface_plot(
            &symbol,
            plot_sender.clone(),
            DataSource::MostRecentOptionsChain,
        )
        .await?;
        return Ok(());
    }

    info!("Starting GUI for ticker input");
    let plotting_task = tokio::spawn(async move {
        while let Some((ticker, data_source)) = ticker_receiver.recv().await {
            info!(
                "Received ticker from GUI: {} with data source: {:?}",
                ticker, data_source
            );
            if let Err(e) =
                run_volatility_surface_plot(&ticker, plot_sender.clone(), data_source).await
            {
                warn!(
                    "Error plotting volatility surface for {} with data source {:?}: {}",
                    ticker, data_source, e
                );
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
        data_source: DataSource::MostRecentOptionsChain, // Default to most recent options chain
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
