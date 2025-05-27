use eframe::egui;
use egui::plot::{Line, Plot, PlotPoints, Points};
use options_rs::api::RestClient;
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::volatility::VolatilitySurface;
use options_rs::models::{OptionContract, OptionQuote};
use options_rs::utils::{self, polars_utils};
use options_rs::models::volatility::ImpliedVolatility;

use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{info, warn, debug};

struct PlotData {
    surface: VolatilitySurface,
    expirations: Vec<chrono::NaiveDate>,
}

struct ExpirationsData {
    expirations: Vec<chrono::NaiveDate>,
}

struct OptionQuoteWithIV {
    quote: OptionQuote,
    implied_volatility: Option<f64>,
}

fn calculate_volatility_surface_with_iv(
    quotes_with_iv: &[OptionQuoteWithIV],
    symbol: &str,
    risk_free_rate: f64,
) -> Result<VolatilitySurface> {
    // Extract quotes
    let quotes: Vec<&OptionQuote> = quotes_with_iv.iter().map(|q| &q.quote).collect();

    // Create implied volatilities
    let mut ivs = Vec::new();
    for (i, q) in quotes_with_iv.iter().enumerate() {
        if let Some(iv_value) = q.implied_volatility {
            // Use the implied volatility from the API if available
            let contract = &q.quote.contract;
            let option_price = q.quote.mid_price();
            let underlying_price = q.quote.underlying_price;
            let time_to_expiration = contract.time_to_expiration();

            // Calculate delta and vega using the implied volatility
            let delta_value = utils::delta(
                underlying_price,
                contract.strike,
                time_to_expiration,
                risk_free_rate,
                iv_value,
                contract.is_call(),
            );

            let vega_value = utils::vega(
                underlying_price,
                contract.strike,
                time_to_expiration,
                risk_free_rate,
                iv_value,
            );

            let iv = ImpliedVolatility {
                contract: contract.clone(),
                value: iv_value,
                underlying_price,
                option_price,
                time_to_expiration,
                delta: delta_value,
                vega: vega_value,
            };
            ivs.push(iv);
        } else {
            // Fall back to calculating IV if not available
            if let Ok(iv) = ImpliedVolatility::from_quote(&quotes[i], risk_free_rate, 0.0) {
                ivs.push(iv);
            }
        }
    }

    if ivs.is_empty() {
        return Err(OptionsError::VolatilityError(
            "No implied volatilities available".to_string(),
        ));
    }

    // Create volatility surface
    let surface = VolatilitySurface::new(symbol.to_string(), &ivs)?;

    Ok(surface)
}

struct VolatilitySurfaceApp {
    ticker_input: String,
    status: String,
    ticker_sender: mpsc::Sender<(String, Option<chrono::NaiveDate>)>,
    plot_receiver: mpsc::Receiver<PlotData>,
    expirations_receiver: mpsc::Receiver<ExpirationsData>,
    surface: Option<VolatilitySurface>,
    expirations: Vec<chrono::NaiveDate>,
    selected_expiration: usize,
    has_expirations: bool,
}


impl eframe::App for VolatilitySurfaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for expirations data
        while let Ok(exp_data) = self.expirations_receiver.try_recv() {
            self.status = "Received expirations data".to_string();
            self.expirations = exp_data.expirations;
            self.has_expirations = true;
            self.selected_expiration = 0;
        }

        // Check for plot data
        while let Ok(plot_data) = self.plot_receiver.try_recv() {
            self.status = "Received new plot data".to_string();
            self.surface = Some(plot_data.surface);
            self.selected_expiration = 0;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Live Volatility Surface Plotter");

            ui.horizontal(|ui| {
                ui.label("Ticker Symbol:");
                ui.text_edit_singleline(&mut self.ticker_input);
            });


            ui.horizontal(|ui| {
                if ui.button("Fetch Options Chain").clicked() {
                    if self.ticker_input.trim().is_empty() {
                        self.status = "Please enter a ticker symbol".to_string();
                    } else {
                        let ticker = self.ticker_input.trim().to_uppercase();
                        self.status = format!("Fetching contracts for {}", ticker);
                        self.has_expirations = false;
                        self.surface = None;

                        if let Err(e) = self.ticker_sender.try_send((ticker, None)) {
                            self.status = format!("Error: {}", e);
                        }
                    }
                }
            });

            ui.separator();
            ui.label(&self.status);
            ui.separator();

            // Show expiration dropdown if we have expirations
            if !self.expirations.is_empty() && self.has_expirations {
                ui.horizontal(|ui| {
                    ui.label("Expiration:");
                    egui::ComboBox::from_id_source("expiry_select")
                        .selected_text(
                            self.expirations[self.selected_expiration]
                                .format("%Y-%m-%d")
                                .to_string(),
                        )
                        .show_ui(ui, |ui| {
                            for (i, exp) in self.expirations.iter().enumerate() {
                                if ui
                                    .selectable_value(
                                        &mut self.selected_expiration,
                                        i,
                                        exp.format("%Y-%m-%d").to_string(),
                                    )
                                    .clicked()
                                {
                                    let ticker = self.ticker_input.trim().to_uppercase();
                                    self.status = format!("Fetching data for {} exp {}", ticker, exp.format("%Y-%m-%d"));
                                    let _ = self.ticker_sender.try_send((ticker, Some(*exp)));
                                }
                            }
                        });
                });
            }

            // Show plot if we have surface data
            if let Some(ref surface) = self.surface {
                _frame.set_window_size(egui::vec2(1000.0, 700.0));

                let exp_dt = chrono::DateTime::<chrono::Utc>::from_utc(
                    self.expirations[self.selected_expiration]
                        .and_hms_opt(16, 0, 0)
                        .unwrap(),
                    chrono::Utc,
                );
                if let Ok((strikes, vols)) = surface.slice_by_expiration(exp_dt)
                {
                    let strike_vec: Vec<f64> = strikes.iter().cloned().collect();
                    let vol_vec: Vec<f64> = vols.iter().cloned().collect();
                    let scatter_points: Vec<[f64; 2]> = strike_vec
                        .iter()
                        .zip(vol_vec.iter())
                        .map(|(s, v)| [*s, *v])
                        .collect();
                    let spline_points = cubic_hermite_spline(&strike_vec, &vol_vec, 10);
                    let line = Line::new(PlotPoints::from(spline_points));
                    let scatter = Points::new(PlotPoints::from(scatter_points));

                    Plot::new("vol_smile_plot")
                        .height(400.0)
                        .width(900.0)
                        .label_formatter(|_, _| String::new())
                        .show(ui, |plot_ui| {
                            plot_ui.line(line);
                            plot_ui.points(scatter);
                        });
                } else {
                    ui.label("Failed to extract smile data");
                }
            } else if !self.has_expirations {
                ui.label("Enter a ticker symbol and click 'Fetch Options Chain' to start.");
                ui.label("Press Ctrl+C in the terminal to exit the application.");
            } else {
                ui.label("Select an expiration date to view the volatility surface.");
            }
        });
    }
}

pub fn parse_options_chain(data: &Value) -> Result<Vec<OptionContract>> {
    let mut options = Vec::new();

    // Try to get option_contracts field first, fall back to results for backward compatibility
    let contracts = data.get("option_contracts").or_else(|| data.get("results"));

    if let Some(results) = contracts {
        if let Some(results_array) = results.as_array() {
            for option_data in results_array {
                // Get the strike price as either a string or a number
                let strike = option_data.get("strike_price").and_then(|p| {
                    if let Some(s) = p.as_str() {
                        // If it's a string, try to parse it as a float
                        s.parse::<f64>().ok()
                    } else {
                        // If it's not a string, try to get it as a float directly
                        p.as_f64()
                    }
                });

                if let (Some(symbol), Some(option_type), Some(strike), Some(expiration)) = (
                    option_data.get("symbol").and_then(|s| s.as_str()),
                    option_data.get("option_type").and_then(|t| t.as_str()),
                    strike,
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
fn cubic_hermite_spline(x: &[f64], y: &[f64], steps: usize) -> Vec<[f64; 2]> {
    let n = x.len();
    if n < 2 {
        return x.iter().zip(y.iter()).map(|(&a, &b)| [a, b]).collect();
    }
    let mut m = vec![0.0; n];
    for i in 0..n {
        if i == 0 {
            m[i] = (y[1] - y[0]) / (x[1] - x[0]);
        } else if i == n - 1 {
            m[i] = (y[n - 1] - y[n - 2]) / (x[n - 1] - x[n - 2]);
        } else {
            m[i] = (y[i + 1] - y[i - 1]) / (x[i + 1] - x[i - 1]);
        }
    }

    let mut result = Vec::new();
    for i in 0..n - 1 {
        let h = x[i + 1] - x[i];
        for s in 0..steps {
            let t = s as f64 / steps as f64;
            let a = 2.0 * t.powi(3) - 3.0 * t.powi(2) + 1.0;
            let b = -2.0 * t.powi(3) + 3.0 * t.powi(2);
            let c = t.powi(3) - 2.0 * t.powi(2) + t;
            let d = t.powi(3) - t.powi(2);
            let y_val = a * y[i] + b * y[i + 1] + c * h * m[i] + d * h * m[i + 1];
            let x_val = x[i] + t * h;
            result.push([x_val, y_val]);
        }
    }
    result.push([x[n - 1], y[n - 1]]);
    result
}

async fn fetch_expirations(
    symbol: &str,
    expirations_sender: mpsc::Sender<ExpirationsData>,
) -> Result<()> {
    let config = Config::from_env()?;
    let rest_client = RestClient::new(config.alpaca.clone());

    // Use current date as expiration_date_gte to get all future expiry dates
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let chain = rest_client
        .get_options_chain(symbol, None, Some(&today), None, None, None, Some(10000), None)
        .await?;

    if chain.results.is_empty() {
        warn!("No option contracts returned for symbol {}", symbol);
        return Ok(());
    }

    let mut contracts = Vec::new();
    for c in &chain.results {
        if let Some(contract) = OptionContract::from_occ_symbol(&c.symbol) {
            contracts.push(contract);
        }
    }

    let mut expirations: Vec<chrono::NaiveDate> = contracts
        .iter()
        .map(|c| c.expiration.date_naive())
        .collect();
    expirations.sort();
    expirations.dedup();
    if expirations.is_empty() {
        warn!("No expirations found for {}", symbol);
        return Ok(());
    }

    // Send expirations to the UI
    let expirations_data = ExpirationsData { expirations };
    expirations_sender
        .send(expirations_data)
        .await
        .map_err(|e| OptionsError::Other(e.to_string()))?;

    Ok(())
}

async fn run_volatility_surface_plot(
    symbol: &str,
    plot_sender: mpsc::Sender<PlotData>,
    expiry: Option<chrono::NaiveDate>,
) -> Result<()> {
    let config = Config::from_env()?;
    let rest_client = RestClient::new(config.alpaca.clone());

    // Use current date as expiration_date_gte to get all future expiry dates
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // Get expirations first to ensure we have them
    let chain = rest_client
        .get_options_chain(symbol, None, Some(&today), None, None, None, Some(10000), None)
        .await?;

    if chain.results.is_empty() {
        warn!("No option contracts returned for symbol {}", symbol);
        return Ok(());
    }

    let mut contracts = Vec::new();
    for c in &chain.results {
        if let Some(contract) = OptionContract::from_occ_symbol(&c.symbol) {
            contracts.push(contract);
        }
    }

    let mut expirations: Vec<chrono::NaiveDate> = contracts
        .iter()
        .map(|c| c.expiration.date_naive())
        .collect();
    expirations.sort();
    expirations.dedup();
    if expirations.is_empty() {
        warn!("No expirations found for {}", symbol);
        return Ok(());
    }

    // Get the latest stock quote using the single quote endpoint
    let quote_resp = rest_client.get_latest_single_stock_quote(symbol, None, None).await?;
    let underlying_price = (quote_resp.quote.bid + quote_resp.quote.ask) / 2.0;

    let chosen = expiry.unwrap_or(expirations[0]);

    // Format the chosen date as YYYY-MM-DD
    let chosen_str = chosen.format("%Y-%m-%d").to_string();

    // Use the new API endpoint to get option chain snapshots
    info!("Fetching option chain snapshots for {} exp {}", symbol, chosen_str);

    // Get the current stock price to set a reasonable strike price range
    let strike_range_factor = 0.5; // 50% above and below current price
    let strike_min = underlying_price * (1.0 - strike_range_factor);
    let strike_max = underlying_price * (1.0 + strike_range_factor);

    info!("Using strike price range: {:.2} to {:.2} for underlying price {:.2}", 
          strike_min, strike_max, underlying_price);

    let snaps = rest_client
        .get_option_chain_snapshots(
            symbol,
            Some("indicative"),
            Some(1000),         // Limit to 1000 snapshots
            None,               // No updated_since filter
            None,               // No page token
            None,               // No option type filter (get both calls and puts)
            Some(strike_min),   // Set minimum strike price
            Some(strike_max),   // Set maximum strike price
            Some(&chosen_str),  // Filter by exact expiration date
            None,               // No expiration_date_gte filter
            None,               // No expiration_date_lte filter
            None,               // No root_symbol filter
        )
        .await?;

    if snaps.snapshots.is_empty() {
        warn!("No option snapshots found for {} exp {}", symbol, chosen);
        return Ok(());
    }

    let mut quotes_with_iv = Vec::new();
    for (occ, snap) in snaps.snapshots {
        if let Some(contract) = OptionContract::from_occ_symbol(&occ) {
            // Skip if expiration date doesn't match (extra safety check)
            if contract.expiration.date_naive() != chosen {
                continue;
            }

            // Try to get bid and ask from last_quote
            let mut bid = snap.last_quote.as_ref().map(|q| q.bid);
            let mut ask = snap.last_quote.as_ref().map(|q| q.ask);
            let mut last_price = snap.last_trade.as_ref().map(|t| t.price);
            let volume = snap.last_trade.as_ref().map(|t| t.size).unwrap_or(0);
            let mut timestamp = snap
                .last_quote
                .as_ref()
                .map(|q| q.t)
                .or_else(|| snap.last_trade.as_ref().map(|t| t.t));

            // Try to get price data from bars if not available from quotes/trades
            let bar = snap
                .dailyBar
                .as_ref()
                .or(snap.minuteBar.as_ref())
                .or(snap.prevDailyBar.as_ref());

            if let Some(bar_data) = bar {
                // Use bar data for missing values
                if last_price.is_none() {
                    last_price = Some(bar_data.c);
                }

                // If bid or ask is missing, derive from close price
                if bid.is_none() || ask.is_none() {
                    // Use close price as mid and create a small spread
                    let mid = bar_data.c;
                    let spread = mid * 0.05; // 5% spread

                    if bid.is_none() {
                        bid = Some(mid - spread/2.0);
                    }

                    if ask.is_none() {
                        ask = Some(mid + spread/2.0);
                    }
                }

                if timestamp.is_none() {
                    timestamp = Some(bar_data.t);
                }
            }

            // Skip if we still don't have enough data
            let Some(bid_value) = bid else { 
                debug!("Skipping contract {} - no bid price available", occ);
                continue;
            };
            let Some(ask_value) = ask else {
                debug!("Skipping contract {} - no ask price available", occ);
                continue;
            };
            let Some(last_price_value) = last_price else {
                debug!("Skipping contract {} - no last price available", occ);
                continue;
            };
            let timestamp = timestamp.unwrap_or_else(chrono::Utc::now);

            // Get implied volatility from the API response if available
            let implied_volatility = snap.impliedVolatility;

            // Create OptionQuote
            let quote = OptionQuote {
                contract,
                bid: bid_value,
                ask: ask_value,
                last: last_price_value,
                volume,
                open_interest: 0,
                underlying_price,
                timestamp,
            };

            // Create OptionQuoteWithIV
            quotes_with_iv.push(OptionQuoteWithIV {
                quote,
                implied_volatility,
            });
        }
    }

    if quotes_with_iv.is_empty() {
        warn!("No quotes collected for {} exp {}", symbol, chosen);
        return Ok(());
    }

    let risk_free_rate = 0.03;

    // Use the new function that uses implied volatility directly from the API
    let surface = calculate_volatility_surface_with_iv(&quotes_with_iv, symbol, risk_free_rate)?;

    let plot_data = PlotData { surface, expirations };
    plot_sender
        .send(plot_data)
        .await
        .map_err(|e| OptionsError::Other(e.to_string()))?;

    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    config.init_logging()?;

    let (ticker_sender, mut ticker_receiver) = mpsc::channel::<(String, Option<chrono::NaiveDate>)>(10);
    let (plot_sender, plot_receiver) = mpsc::channel::<PlotData>(10);
    let (expirations_sender, expirations_receiver) = mpsc::channel::<ExpirationsData>(10);

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let symbol = args[1].clone();
        info!("Ticker provided as command-line argument: {}", symbol);
        // Default to most recent options chain for command-line usage
        fetch_expirations(&symbol, expirations_sender.clone()).await?;
        // Wait a bit for expirations to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        run_volatility_surface_plot(
            &symbol,
            plot_sender.clone(),
            None,
        )
        .await?;
        return Ok(());
    }

    info!("Starting GUI for ticker input");
    let plotting_task = tokio::spawn(async move {
        while let Some((ticker, expiry)) = ticker_receiver.recv().await {
            info!("Received request for {} exp {:?}", ticker, expiry);
            if expiry.is_none() {
                // First fetch expirations
                if let Err(e) = fetch_expirations(&ticker, expirations_sender.clone()).await {
                    warn!("Error fetching expirations for {}: {}", ticker, e);
                }
            } else {
                // Then fetch surface data with selected expiry
                if let Err(e) = run_volatility_surface_plot(&ticker, plot_sender.clone(), expiry).await {
                    warn!("Error plotting volatility surface for {}: {}", ticker, e);
                }
            }
        }
    });

    let app = VolatilitySurfaceApp {
        ticker_input: String::new(),
        status: "Enter a ticker symbol and click 'Plot Volatility Surface'".to_string(),
        ticker_sender,
        plot_receiver,
        expirations_receiver,
        surface: None,
        expirations: Vec::new(),
        selected_expiration: 0,
        has_expirations: false,
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
