use eframe::egui;
use egui_plot::{GridMark, Line, Plot, PlotPoints, Points, VLine};
use options_rs::api::OptionGreeks;
use options_rs::api::RestClient;
use options_rs::config::Config;
use options_rs::error::{OptionsError, Result};
use options_rs::models::volatility::ImpliedVolatility;
use options_rs::models::volatility::VolatilitySurface;
use options_rs::models::{OptionContract, OptionQuote};
use options_rs::utils::{self};

use chrono::TimeZone;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

static SURFACE_CACHE: Lazy<
    DashMap<(String, chrono::DateTime<chrono::Utc>), Arc<VolatilitySurface>>,
> = Lazy::new(|| DashMap::new());

static RISK_FREE_RATE_CACHE: Lazy<DashMap<chrono::NaiveDate, f64>> = Lazy::new(|| DashMap::new());

static CONTRACT_METADATA_CACHE: Lazy<DashMap<chrono::NaiveDate, DashMap<String, OptionContract>>> =
    Lazy::new(|| DashMap::new());

static RATE_LIMIT_RESET: Lazy<std::sync::Mutex<Option<chrono::DateTime<chrono::Utc>>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

struct PlotData {
    surface: Arc<VolatilitySurface>,
    expirations: Vec<chrono::NaiveDate>,
    underlying_price: f64,
}

struct ExpirationsData {
    expirations: Vec<chrono::NaiveDate>,
}

#[derive(Clone)]
struct OptionQuoteWithIV {
    quote: OptionQuote,
    implied_volatility: Option<f64>,
    greeks: Option<OptionGreeks>,
}

fn calculate_volatility_surface_with_iv(
    quotes_with_iv: &[OptionQuoteWithIV],
    symbol: &str,
    risk_free_rate: f64,
) -> Result<VolatilitySurface> {
    let quotes: Vec<&OptionQuote> = quotes_with_iv.iter().map(|q| &q.quote).collect();

    let mut ivs = Vec::new();
    for (i, q) in quotes_with_iv.iter().enumerate() {
        if let Some(iv_value) = q.implied_volatility {
            let contract = &q.quote.contract;
            let option_price = q.quote.mid_price();
            let underlying_price = q.quote.underlying_price;
            let time_to_expiration = contract.time_to_expiration();

            let (delta_value, vega_value) = if let Some(g) = &q.greeks {
                (g.delta, g.vega)
            } else {
                let delta = utils::delta(
                    underlying_price,
                    contract.strike,
                    time_to_expiration,
                    risk_free_rate,
                    iv_value,
                    contract.is_call(),
                );

                let vega = utils::vega(
                    underlying_price,
                    contract.strike,
                    time_to_expiration,
                    risk_free_rate,
                    iv_value,
                );

                (delta, vega)
            };

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

    let surface = VolatilitySurface::new(symbol.to_string(), &ivs)?;

    Ok(surface)
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum ViewMode {
    VolatilitySkew,
    TermStructure,
}

struct VolatilitySurfaceApp {
    ticker_input: String,
    status: String,
    ticker_sender: mpsc::Sender<(String, Option<chrono::NaiveDate>, Option<ViewMode>)>,
    plot_receiver: mpsc::Receiver<PlotData>,
    expirations_receiver: mpsc::Receiver<ExpirationsData>,
    surface: Option<Arc<VolatilitySurface>>,
    expirations: Vec<chrono::NaiveDate>,
    selected_expiration: usize,
    has_expirations: bool,
    expiry_selected: bool,
    underlying_price: Option<f64>,
    view_mode: ViewMode,
    selected_strike: Option<f64>,
}

impl eframe::App for VolatilitySurfaceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(exp_data) = self.expirations_receiver.try_recv() {
            self.status = "Received expirations data".to_string();
            self.expirations = exp_data.expirations;
            self.has_expirations = true;
            self.selected_expiration = 0;
            self.expiry_selected = false;

            ctx.request_repaint();
        }

        while let Ok(plot_data) = self.plot_receiver.try_recv() {
            self.status = "Received new plot data".to_string();
            self.surface = Some(plot_data.surface);
            self.underlying_price = Some(plot_data.underlying_price);

            ctx.request_repaint();
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
                        self.underlying_price = None;
                        self.expiry_selected = false;

                        if let Err(e) = self.ticker_sender.try_send((ticker, None, None)) {
                            self.status = format!("Error: {}", e);
                        }
                    }
                }
            });

            ui.separator();
            ui.label(&self.status);
            ui.separator();

            if !self.expirations.is_empty() && self.has_expirations {
                ui.horizontal(|ui| {
                    ui.label("View Mode:");
                    let old_view_mode = self.view_mode;
                    ui.radio_value(&mut self.view_mode, ViewMode::VolatilitySkew, "Volatility Skew");
                    ui.radio_value(&mut self.view_mode, ViewMode::TermStructure, "Term Structure");

                    if old_view_mode != self.view_mode {
                        if self.view_mode == ViewMode::TermStructure && !self.ticker_input.trim().is_empty() {
                            let ticker = self.ticker_input.trim().to_uppercase();
                            self.status = format!("Fetching all option data for {}", ticker);
                            self.surface = None;
                            ctx.request_repaint();
                            if let Err(e) = self.ticker_sender.try_send((ticker, None, Some(self.view_mode))) {
                                self.status = format!("Error: {}", e);
                            }
                        }
                    }
                });

                if self.view_mode == ViewMode::VolatilitySkew {
                    ui.horizontal(|ui| {
                        ui.label("Expiration:");
                        egui::ComboBox::from_id_source("expiry_select")
                            .selected_text(
                                if self.expiry_selected {
                                    self.expirations[self.selected_expiration]
                                        .format("%Y-%m-%d")
                                        .to_string()
                                } else {
                                    "select expiry".to_string()
                                }
                            )
                            .show_ui(ui, |ui| {
                                for (i, exp) in self.expirations.iter().enumerate() {
                                    let response = ui
                                        .selectable_value(
                                            &mut self.selected_expiration,
                                            i,
                                            exp.format("%Y-%m-%d").to_string(),
                                        );

                                    if response.clicked() {
                                        self.expiry_selected = true;
                                        let ticker = self.ticker_input.trim().to_uppercase();
                                        self.status = format!("Fetching data for {} exp {}", ticker, exp.format("%Y-%m-%d"));
                                        self.surface = None;
                                        ctx.request_repaint();
                                        if let Err(e) = self.ticker_sender.try_send((ticker, Some(*exp), Some(self.view_mode))) {
                                            self.status = format!("Error: {}", e);
                                        }
                                    }
                                }
                            });
                    });
                } else if self.view_mode == ViewMode::TermStructure {
                    ui.horizontal(|ui| {
                        ui.label("Strike Price:");

                        if let Some(ref surface) = self.surface {
                            let strikes: Vec<f64> = surface.strikes.clone();

                            if !strikes.is_empty() {
                                if self.selected_strike.is_none() {
                                    if let Some(underlying) = self.underlying_price {
                                        let closest_strike = strikes.iter()
                                            .min_by(|a, b| {
                                                let a_diff = (*a - underlying).abs();
                                                let b_diff = (*b - underlying).abs();
                                                a_diff.partial_cmp(&b_diff).unwrap_or(std::cmp::Ordering::Equal)
                                            })
                                            .cloned();
                                        self.selected_strike = closest_strike;
                                    } else {

                                        self.selected_strike = strikes.get(strikes.len() / 2).cloned();
                                    }
                                }

                                egui::ComboBox::from_id_source("strike_select")
                                    .selected_text(
                                        if let Some(strike) = self.selected_strike {
                                            format!("{:.2}", strike)
                                        } else {
                                            "select strike".to_string()
                                        }
                                    )
                                    .show_ui(ui, |ui| {
                                        for strike in &strikes {
                                            let response = ui.selectable_value(
                                                &mut self.selected_strike,
                                                Some(*strike),
                                                format!("{:.2}", strike),
                                            );

                                            if response.clicked() {

                                                ctx.request_repaint();
                                            }
                                        }
                                    });
                            } else {
                                ui.label("No strike prices available. Please try a different symbol.");
                            }
                        } else {

                            ui.label("Loading option data... Strike prices will be available soon.");
                        }
                    });
                }
            }


            if let Some(ref surface) = self.surface {



                if self.selected_expiration >= self.expirations.len() && !self.expirations.is_empty() {
                    self.selected_expiration = 0;
                }


                let can_show_plot = match self.view_mode {

                    ViewMode::VolatilitySkew => !self.expirations.is_empty() && self.expiry_selected,

                    ViewMode::TermStructure => self.selected_strike.is_some(),
                };

                if can_show_plot {

                    match self.view_mode {
                        ViewMode::VolatilitySkew => {

                            let exp_dt = chrono::Utc.from_utc_datetime(
                                &self.expirations[self.selected_expiration]
                                    .and_hms_opt(16, 0, 0)
                                    .unwrap()
                            );

                            if let Ok((strikes, vols)) = surface.slice_by_expiration(exp_dt) {
                                let strike_vec: Vec<f64> = strikes.iter().cloned().collect();
                                let vol_vec: Vec<f64> = vols.iter().cloned().collect();


                                let underlying = self.underlying_price.unwrap_or(0.0);


                                let mut strike_vol_dist: Vec<(f64, f64, f64)> = strike_vec
                                    .iter()
                                    .zip(vol_vec.iter())
                                    .map(|(s, v)| (*s, *v, (s - underlying).abs()))
                                    .collect();


                                strike_vol_dist.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));


                                let plot_id = format!("vol_smile_plot_{}_{}",
                                                     self.ticker_input,
                                                     self.expirations[self.selected_expiration].format("%Y-%m-%d"));


                                let mut plot = Plot::new(plot_id)
                                    .height(400.0)
                                    .width(900.0)
                                    .label_formatter(|_, _| String::new());


                                if underlying > 0.0 {

                                    let strike_range = 0.2 * underlying;
                                    let min_strike = underlying - strike_range;
                                    let max_strike = underlying + strike_range;



                                    plot = plot.include_x(underlying)
                                               .include_x(min_strike)
                                               .include_x(max_strike);


                                    let step = strike_range / 5.0;
                                    for i in 1..5 {
                                        plot = plot.include_x(underlying - i as f64 * step)
                                                   .include_x(underlying + i as f64 * step);
                                    }
                                }

                                plot.show(ui, |plot_ui| {

                                    let spline_points = cubic_hermite_spline(&strike_vec, &vol_vec, 10);
                                    let line = Line::new(PlotPoints::from(spline_points));
                                    plot_ui.line(line);



                                    let all_points: Vec<[f64; 2]> = strike_vol_dist
                                        .iter()
                                        .map(|(s, v, _)| [*s, *v])
                                        .collect();

                                    let scatter = Points::new(PlotPoints::from(all_points))
                                        .radius(3.0)
                                        .color(egui::Color32::from_rgb(139, 0, 0));
                                    plot_ui.points(scatter);
                                });
                            } else {
                                ui.label("Failed to extract smile data for the selected expiration date.");
                                ui.label("Try selecting a different expiration date.");
                            }
                        },
                        ViewMode::TermStructure => {

                            if let Some(strike) = self.selected_strike {
                                if let Ok((times, vols)) = surface.slice_by_strike(strike) {
                                    let vol_vec: Vec<f64> = vols.iter().cloned().collect();


                                    let today = chrono::Utc::now().date_naive();
                                    let date_offsets: Vec<f64> = surface.expirations
                                        .iter()
                                        .map(|d| (d.date_naive().signed_duration_since(today)).num_days() as f64)
                                        .collect();


                                    let mut points: Vec<[f64; 2]> = date_offsets
                                        .iter()
                                        .zip(vol_vec.iter())
                                        .map(|(dx, v)| [*dx, *v])
                                        .collect();


                                    points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));


                                    let (x_vals, y_vals): (Vec<f64>, Vec<f64>) = points
                                        .iter()
                                        .map(|p| (p[0], p[1]))
                                        .unzip();


                                    let plot_id = format!("term_structure_plot_{}_{}",
                                                         self.ticker_input, strike);


                                    let plot = Plot::new(plot_id)
                                        .height(400.0)
                                        .width(900.0)
                                        .include_x(0.0)
                                        .x_axis_formatter(move |grid_mark: GridMark, _range: &std::ops::RangeInclusive<f64>| {

                                            let d = today + chrono::Duration::days(grid_mark.value.round() as i64);
                                            d.format("%b %d").to_string()
                                        });

                                    plot.show(ui, |plot_ui| {

                                        plot_ui.vline(VLine::new(0.0));


                                        let spline_points = cubic_hermite_spline(&x_vals, &y_vals, 10);
                                        let line = Line::new(PlotPoints::from(spline_points));
                                        plot_ui.line(line);


                                        let scatter = Points::new(PlotPoints::from(points))
                                            .radius(3.0)
                                            .color(egui::Color32::from_rgb(0, 100, 139));
                                        plot_ui.points(scatter);


                                        ctx.request_repaint();
                                    });
                                } else {
                                    ui.label("Failed to extract term structure data for the selected strike price.");
                                    ui.label("Try selecting a different strike price.");
                                }
                            } else {
                                ui.label("Please select a strike price to view the term structure.");
                            }
                        }
                    }
                } else {

                    match self.view_mode {
                        ViewMode::VolatilitySkew => {
                            if !self.expiry_selected {
                                ui.label("Please select an expiry from the dropdown above to render the volatility skew.");
                            } else {
                                ui.label("No expiration dates available. Please try a different symbol.");
                            }
                        },
                        ViewMode::TermStructure => {
                            if self.selected_strike.is_none() {
                                ui.label("Please select a strike price from the dropdown above to render the term structure.");
                            } else {
                                ui.label("Failed to render term structure. Please try a different symbol or strike price.");
                            }
                        }
                    }
                }
            } else if !self.has_expirations {
                ui.label("Enter a ticker symbol and click 'Fetch Options Chain' to start.");
                ui.label("Press Ctrl+C in the terminal to exit the application.");
            } else {
                ui.label("Select an expiry from the dropdown to view the volatility surface.");
            }
        });
    }
}

pub fn parse_options_chain(data: &Value) -> Result<Vec<OptionContract>> {
    let mut options = Vec::new();

    let contracts = data.get("option_contracts").or_else(|| data.get("results"));

    if let Some(results) = contracts {
        if let Some(results_array) = results.as_array() {
            for option_data in results_array {
                let strike = option_data.get("strike_price").and_then(|p| {
                    if let Some(s) = p.as_str() {
                        s.parse::<f64>().ok()
                    } else {
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

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let chain = rest_client
        .get_options_chain(
            symbol,
            None,
            Some(&today),
            None,
            None,
            None,
            Some(10000),
            None,
        )
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
    view_mode: Option<ViewMode>,
) -> Result<()> {
    let config = Config::from_env()?;
    let rest_client = RestClient::new(config.alpaca.clone());

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let chain = rest_client
        .get_options_chain(
            symbol,
            None,
            Some(&today),
            None,
            None,
            None,
            Some(10000),
            None,
        )
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

    let quote_resp = rest_client
        .get_latest_single_stock_quote(symbol, None, None)
        .await?;
    let underlying_price = (quote_resp.quote.bid + quote_resp.quote.ask) / 2.0;

    let strike_range_factor = 0.5;
    let strike_min = underlying_price * (1.0 - strike_range_factor);
    let strike_max = underlying_price * (1.0 + strike_range_factor);

    info!(
        "Using strike price range: {:.2} to {:.2} for underlying price {:.2}",
        strike_min, strike_max, underlying_price
    );

    let snaps = if let Some(ViewMode::TermStructure) = view_mode {
        info!(
            "Term structure view: Fetching all option chain snapshots for {}",
            symbol
        );

        rest_client
            .get_option_chain_snapshots(
                symbol,
                Some("indicative"),
                Some(1000),
                None,
                None,
                None,
                Some(strike_min),
                Some(strike_max),
                None,
                Some(&today),
                None,
                None,
            )
            .await?
    } else if let Some(chosen) = expiry {
        let chosen_str = chosen.format("%Y-%m-%d").to_string();
        info!(
            "Volatility skew view: Fetching option chain snapshots for {} exp {}",
            symbol, chosen_str
        );

        rest_client
            .get_option_chain_snapshots(
                symbol,
                Some("indicative"),
                Some(1000),
                None,
                None,
                None,
                Some(strike_min),
                Some(strike_max),
                Some(&chosen_str),
                None,
                None,
                None,
            )
            .await?
    } else {
        info!("Fetching all option chain snapshots for {}", symbol);

        rest_client
            .get_option_chain_snapshots(
                symbol,
                Some("indicative"),
                Some(1000),
                None,
                None,
                None,
                Some(strike_min),
                Some(strike_max),
                None,
                Some(&today),
                None,
                None,
            )
            .await?
    };

    if snaps.snapshots.is_empty() {
        if let Some(chosen) = expiry {
            warn!("No option snapshots found for {} exp {}", symbol, chosen);
        } else {
            warn!("No option snapshots found for {}", symbol);
        }
        return Ok(());
    }

    let mut quotes_with_iv = Vec::new();
    for (occ, snap) in snaps.snapshots {
        if let Some(contract) = OptionContract::from_occ_symbol(&occ) {
            if let Some(chosen) = expiry {
                if let Some(ViewMode::TermStructure) = view_mode {
                } else if contract.expiration.date_naive() != chosen {
                    continue;
                }
            }

            let mut bid = snap.last_quote.as_ref().map(|q| q.bid);
            let mut ask = snap.last_quote.as_ref().map(|q| q.ask);
            let mut last_price = snap.last_trade.as_ref().map(|t| t.price);
            let volume = snap.last_trade.as_ref().map(|t| t.size).unwrap_or(0);
            let mut timestamp = snap
                .last_quote
                .as_ref()
                .map(|q| q.t)
                .or_else(|| snap.last_trade.as_ref().map(|t| t.t));

            let bar = snap
                .daily_bar
                .as_ref()
                .or(snap.minute_bar.as_ref())
                .or(snap.prev_daily_bar.as_ref());

            if let Some(bar_data) = bar {
                if last_price.is_none() {
                    last_price = Some(bar_data.c);
                }

                if bid.is_none() || ask.is_none() {
                    let mid = bar_data.c;
                    let spread = mid * 0.05;

                    if bid.is_none() {
                        bid = Some(mid - spread / 2.0);
                    }

                    if ask.is_none() {
                        ask = Some(mid + spread / 2.0);
                    }
                }

                if timestamp.is_none() {
                    timestamp = Some(bar_data.t);
                }
            }

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

            let implied_volatility = snap.implied_volatility;
            let greeks = snap.greeks;

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

            quotes_with_iv.push(OptionQuoteWithIV {
                quote,
                implied_volatility,
                greeks,
            });
        }
    }

    if quotes_with_iv.is_empty() {
        if let Some(chosen) = expiry {
            warn!("No quotes collected for {} exp {}", symbol, chosen);
        } else {
            warn!("No quotes collected for {}", symbol);
        }
        return Ok(());
    }

    let risk_free_rate = 0.03;

    let timestamp = chrono::Utc::now();
    let cache_key = (symbol.to_string(), timestamp);

    let surface = if let Some(cached_surface) = SURFACE_CACHE.get(&cache_key) {
        debug!("Using cached volatility surface for {}", symbol);
        cached_surface.clone()
    } else {
        debug!("Calculating new volatility surface for {}", symbol);

        let quotes_with_iv_clone = quotes_with_iv.clone();
        let symbol_clone = symbol.to_string();
        let surface = tokio::task::spawn_blocking(move || {
            calculate_volatility_surface_with_iv(
                &quotes_with_iv_clone,
                &symbol_clone,
                risk_free_rate,
            )
        })
        .await
        .map_err(|e| {
            OptionsError::Other(format!("Failed to calculate volatility surface: {}", e))
        })??;

        let arc_surface = Arc::new(surface);
        SURFACE_CACHE.insert(cache_key, arc_surface.clone());
        arc_surface
    };

    let plot_data = PlotData {
        surface,
        expirations,
        underlying_price,
    };
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

    let (ticker_sender, mut ticker_receiver) =
        mpsc::channel::<(String, Option<chrono::NaiveDate>, Option<ViewMode>)>(10);
    let (plot_sender, plot_receiver) = mpsc::channel::<PlotData>(10);
    let (expirations_sender, expirations_receiver) = mpsc::channel::<ExpirationsData>(10);

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let symbol = args[1].clone();
        info!("Ticker provided as command-line argument: {}", symbol);

        fetch_expirations(&symbol, expirations_sender.clone()).await?;

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        run_volatility_surface_plot(&symbol, plot_sender.clone(), None, None).await?;
        return Ok(());
    }

    info!("Starting GUI for ticker input");
    let _plotting_task = tokio::spawn(async move {
        while let Some((ticker, expiry, view_mode)) = ticker_receiver.recv().await {
            info!(
                "Received request for {} exp {:?} view mode {:?}",
                ticker, expiry, view_mode
            );
            if expiry.is_none() {
                if let Err(e) = fetch_expirations(&ticker, expirations_sender.clone()).await {
                    warn!("Error fetching expirations for {}: {}", ticker, e);
                }

                if let Some(ViewMode::TermStructure) = view_mode {
                    info!(
                        "Term structure view selected, fetching all option data for {}",
                        ticker
                    );
                    if let Err(e) =
                        run_volatility_surface_plot(&ticker, plot_sender.clone(), None, view_mode)
                            .await
                    {
                        warn!("Error plotting term structure for {}: {}", ticker, e);
                    }
                }
            } else {
                if let Err(e) =
                    run_volatility_surface_plot(&ticker, plot_sender.clone(), expiry, view_mode)
                        .await
                {
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
        expiry_selected: false,
        underlying_price: None,
        view_mode: ViewMode::VolatilitySkew,
        selected_strike: None,
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Live Volatility Surface Plotter",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
    .map_err(|e| {
        let err_msg = format!("Failed to start GUI: {}", e);
        warn!("{}", err_msg);
        OptionsError::Other(err_msg)
    })?;

    info!("shutting down");
    Ok(())
}
