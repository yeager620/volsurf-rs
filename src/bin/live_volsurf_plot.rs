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

struct PlotData {
    surface: VolatilitySurface,
    expirations: Vec<chrono::NaiveDate>,
}

struct VolatilitySurfaceApp {
    ticker_input: String,
    status: String,
    ticker_sender: mpsc::Sender<(String, Option<chrono::NaiveDate>)>,
    plot_receiver: mpsc::Receiver<PlotData>,
    surface: Option<VolatilitySurface>,
    expirations: Vec<chrono::NaiveDate>,
    selected_expiration: usize,
}


impl eframe::App for VolatilitySurfaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(plot_data) = self.plot_receiver.try_recv() {
            self.status = "Received new plot data".to_string();
            self.surface = Some(plot_data.surface);
            self.expirations = plot_data.expirations;
            self.selected_expiration = 0;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Live Volatility Surface Plotter");

            ui.horizontal(|ui| {
                ui.label("Ticker Symbol:");
                ui.text_edit_singleline(&mut self.ticker_input);
            });


            ui.horizontal(|ui| {
                if ui.button("Plot Volatility Surface").clicked() {
                    if self.ticker_input.trim().is_empty() {
                        self.status = "Please enter a ticker symbol".to_string();
                    } else {
                        let ticker = self.ticker_input.trim().to_uppercase();
                        self.status = format!("Fetching contracts for {}", ticker);

                        if let Err(e) = self.ticker_sender.try_send((ticker, None)) {
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
                                    let _ = self.ticker_sender.try_send((ticker, Some(*exp)));
                                }
                            }
                        });
                });

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
    expiry: Option<chrono::NaiveDate>,
) -> Result<()> {
    let config = Config::from_env()?;
    let rest_client = RestClient::new(config.alpaca.clone());

    let chain = rest_client
        .get_options_chain(symbol, None, None, None, None, None, Some(10000), None)
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

    let quote_resp = rest_client.get_latest_stock_quotes(&[symbol]).await?;
    let underlying_price = quote_resp
        .quotes
        .get(symbol)
        .map(|q| (q.bid + q.ask) / 2.0)
        .unwrap_or(0.0);

    let chosen = expiry.unwrap_or(expirations[0]);
    let mut option_symbols = Vec::new();
    for c in &contracts {
        if c.expiration.date_naive() == chosen {
            if (c.strike - underlying_price).abs() <= underlying_price * 0.30 {
                option_symbols.push(c.option_symbol.clone());
            }
        }
    }

    if option_symbols.is_empty() {
        warn!("No option symbols found for {} exp {}", symbol, chosen);
        return Ok(());
    }

    let symbol_refs: Vec<&str> = option_symbols.iter().map(String::as_str).collect();
    let snaps = rest_client
        .get_option_snapshots(&symbol_refs, Some("indicative"), None, None, None)
        .await?;

    let mut quotes = Vec::new();
    for (occ, snap) in snaps.snapshots {
        if let Some(contract) = OptionContract::from_occ_symbol(&occ) {
            let bid = snap.last_quote.as_ref().map(|q| q.bid);
            let ask = snap.last_quote.as_ref().map(|q| q.ask);
            if bid.is_none() || ask.is_none() { continue; }

            let mut last_price = snap.last_trade.as_ref().map(|t| t.price);
            let mut volume = snap.last_trade.as_ref().map(|t| t.size).unwrap_or(0);
            let mut timestamp = snap
                .last_quote
                .as_ref()
                .map(|q| q.t)
                .or_else(|| snap.last_trade.as_ref().map(|t| t.t));

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

            let Some(bid) = bid else { continue };
            let Some(ask) = ask else { continue };
            let Some(last_price) = last_price else { continue };
            let timestamp = timestamp.unwrap_or_else(chrono::Utc::now);

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

    if quotes.is_empty() {
        warn!("No quotes collected for {} exp {}", symbol, chosen);
        return Ok(());
    }

    let risk_free_rate = 0.03;
    let surface = polars_utils::calculate_volatility_surface_with_polars(&quotes, symbol, risk_free_rate)?;

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

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let symbol = args[1].clone();
        info!("Ticker provided as command-line argument: {}", symbol);
        // Default to most recent options chain for command-line usage
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
            if let Err(e) =
                run_volatility_surface_plot(&ticker, plot_sender.clone(), expiry).await
            {
                warn!("Error plotting volatility surface for {}: {}", ticker, e);
            }
        }
    });

    let app = VolatilitySurfaceApp {
        ticker_input: String::new(),
        status: "Enter a ticker symbol and click 'Plot Volatility Surface'".to_string(),
        ticker_sender,
        plot_receiver,
        surface: None,
        expirations: Vec::new(),
        selected_expiration: 0,
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
