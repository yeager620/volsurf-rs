use eframe::egui;
use egui::plot::{Line, Plot, PlotPoints, Points};
use options_rs::api::RestClient;
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::volatility::VolatilitySurface;
use options_rs::models::{OptionContract, OptionQuote};
use options_rs::utils::polars_utils;

use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{info, warn};
use std::path::Path;

struct PlotData {
    surface: VolatilitySurface,
}

struct VolatilitySurfaceApp {
    ticker_input: String,
    status: String,
    ticker_sender: mpsc::Sender<(String, DataSource)>,
    plot_receiver: mpsc::Receiver<PlotData>,
    surface: Option<VolatilitySurface>,
    selected_expiration: usize,
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

            if let Some(ref surface) = self.surface {
                _frame.set_window_size(egui::vec2(1000.0, 700.0));

                ui.horizontal(|ui| {
                    ui.label("Expiration:");
                    egui::ComboBox::from_id_source("expiry_select")
                        .selected_text(
                            surface.expirations[self.selected_expiration]
                                .format("%Y-%m-%d")
                                .to_string(),
                        )
                        .show_ui(ui, |ui| {
                            for (i, exp) in surface.expirations.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected_expiration,
                                    i,
                                    exp.format("%Y-%m-%d").to_string(),
                                );
                            }
                        });
                });

                if let Ok((strikes, vols)) =
                    surface.slice_by_expiration(surface.expirations[self.selected_expiration])
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
            } else {
                ui.label("Waiting for plot data...");
                ui.label("Press Ctrl+C in the terminal to exit the application.");
            }
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

async fn run_volatility_surface_plot(
    symbol: &str,
    plot_sender: mpsc::Sender<PlotData>,
    data_source: DataSource,
) -> Result<()> {
    if data_source == DataSource::LiveUpdates {
        return Err(OptionsError::Other(
            "Live update source is not implemented in this example".to_string(),
        ));
    }

    // Check if we have a cached volatility surface for this symbol
    let cache_dir = "cache";
    let cache_file = format!("{}/vol_surface_{}.parquet", cache_dir, symbol);

    // Try to load from cache first if the file exists
    if Path::new(&cache_file).exists() {
        info!("Found cached volatility surface for {}", symbol);
        match polars_utils::load_dataframe_from_parquet(&cache_file) {
            Ok(df) => {
                info!("Loaded volatility surface from cache");
                match polars_utils::dataframe_to_volatility_surface(&df, symbol) {
                    Ok(surface) => {
                        // Check if the cache is recent (less than 1 hour old)
                        let now = chrono::Utc::now();
                        let cache_age = now.signed_duration_since(surface.timestamp);
                        if cache_age.num_minutes() < 60 {
                            info!("Using cached volatility surface (age: {} minutes)", cache_age.num_minutes());
                            let plot_data = PlotData { surface };
                            plot_sender
                                .send(plot_data)
                                .await
                                .map_err(|e| OptionsError::Other(e.to_string()))?;
                            return Ok(());
                        } else {
                            info!("Cached volatility surface is too old ({} minutes), fetching fresh data", 
                                  cache_age.num_minutes());
                        }
                    }
                    Err(e) => {
                        warn!("Failed to convert cached DataFrame to volatility surface: {}", e);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to load cached volatility surface: {}", e);
            }
        }
    }

    let config = Config::from_env()?;
    let rest_client = RestClient::new(config.alpaca.clone());

    info!("Fetching option chain snapshots for {}", symbol);
    let chain_resp = rest_client
        .get_option_chain_snapshots(
            symbol,
            Some("indicative"), // feed
            Some(1000),         // limit
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

    if chain_resp.snapshots.is_empty() {
        warn!("No option snapshots returned for symbol {}", symbol);
        return Ok(());
    }

    use std::collections::HashMap;
    use chrono::NaiveDate;

    let mut by_exp: HashMap<NaiveDate, Vec<String>> = HashMap::new();
    for (occ, snap) in &chain_resp.snapshots {
        if let Ok(exp) = NaiveDate::parse_from_str(&snap.expiration_date, "%Y-%m-%d") {
            by_exp.entry(exp).or_default().push(occ.clone());
        }
    }

    let Some((&chosen_expiry, symbols_vec)) = by_exp.iter().min_by_key(|(d, _)| *d) else {
        warn!("No expirations found for symbol {}", symbol);
        return Ok(());
    };

    let symbol_list: Vec<&str> = symbols_vec.iter().map(String::as_str).collect();

    let snap_resp = rest_client
        .get_option_snapshots(&symbol_list, Some("indicative"), None, None, None)
        .await?;

    let spot_json = rest_client.get_stock_snapshot(symbol).await?;
    let mut underlying_price = 0.0;
    if let Some(val) = spot_json.get(symbol) {
        underlying_price = val
            .get("dailyBar")
            .and_then(|v| v.get("c"))
            .and_then(|v| v.as_f64())
            .or_else(|| {
                val.get("latestTrade")
                    .and_then(|v| v.get("p"))
                    .and_then(|v| v.as_f64())
            })
            .unwrap_or(0.0);
    }

    let mut quotes = Vec::new();
    for (occ, snap) in snap_resp.snapshots {
        if let Some(contract) = OptionContract::from_occ_symbol(&occ) {
            if contract.expiration.date_naive() != chosen_expiry {
                continue;
            }
            let mut bid: Option<f64> = None;
            let mut ask: Option<f64> = None;
            let mut last_price: Option<f64> = None;
            let mut volume: u64 = 0;
            let mut timestamp: Option<chrono::DateTime<chrono::Utc>> = None;

            if let Some(q) = snap.last_quote {
                bid = Some(q.bid);
                ask = Some(q.ask);
                timestamp = Some(q.t);
            }

            if let Some(t) = snap.last_trade {
                last_price = Some(t.price);
                volume = t.size;
                if timestamp.is_none() {
                    timestamp = Some(t.t);
                }
            }

            if bid.is_some() && ask.is_some() {
                let spread = ask.unwrap() - bid.unwrap();
                let mid = (ask.unwrap() + bid.unwrap()) / 2.0;
                if mid <= 0.0 || spread / mid > 0.05 {
                    continue;
                }
            } else {
                continue;
            }

            if last_price.is_none() {
                if let Some(bar) = snap
                    .dailyBar
                    .as_ref()
                    .or(snap.minuteBar.as_ref())
                    .or(snap.prevDailyBar.as_ref())
                {
                    last_price = Some(bar.c);
                    if timestamp.is_none() {
                        timestamp = Some(bar.t);
                    }
                }
            }

            if timestamp.is_none() {
                timestamp = Some(chrono::Utc::now());
            }

            if let (Some(bid), Some(ask), Some(last_price), Some(timestamp)) =
                (bid, ask, last_price, timestamp)
            {
                quotes.push(OptionQuote {
                    contract,
                    bid,
                    ask,
                    last: last_price,
                    volume,
                    open_interest: 0,
                    underlying_price,
                    timestamp,
                });
            }
        }
    }

    // Use Polars for efficient processing
    info!("Processing {} option quotes with Polars", quotes.len());

    // Convert quotes to DataFrame for analysis
    let quotes_df = match polars_utils::quotes_to_dataframe(&quotes) {
        Ok(df) => {
            info!("Created DataFrame with {} rows", df.height());
            df
        },
        Err(e) => {
            warn!("Failed to create DataFrame from quotes: {}", e);
            return Err(e);
        }
    };

    // Calculate volatility surface using Polars
    let risk_free_rate = 0.03;
    let surface = match polars_utils::calculate_volatility_surface_with_polars(&quotes, symbol, risk_free_rate) {
        Ok(surface) => {
            info!("Calculated volatility surface with {} expirations and {} strikes", 
                  surface.expirations.len(), surface.strikes.len());
            surface
        },
        Err(e) => {
            warn!("Failed to calculate volatility surface: {}", e);
            return Err(e);
        }
    };

    // Cache the volatility surface for future use
    if let Ok(surface_df) = polars_utils::volatility_surface_to_dataframe(&surface) {
        // Create cache directory if it doesn't exist
        if !Path::new(cache_dir).exists() {
            if let Err(e) = std::fs::create_dir_all(cache_dir) {
                warn!("Failed to create cache directory: {}", e);
            }
        }

        // Save to Parquet file
        if let Err(e) = polars_utils::cache_dataframe_to_parquet(&surface_df, &cache_file) {
            warn!("Failed to cache volatility surface: {}", e);
        } else {
            info!("Cached volatility surface to {}", cache_file);
        }
    }

    let plot_data = PlotData { surface };
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
        surface: None,
        selected_expiration: 0,
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
