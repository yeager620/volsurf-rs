use crate::api::RestClient;
use crate::error::{OptionsError, Result};
use crate::models::volatility::VolatilitySurface;
use crate::models::{ImpliedVolatility, OptionContract, OptionType};
use chrono::Utc;
use minifb::{Key, Window, WindowOptions};
use plotters::coord::Shift;
use plotters::prelude::*;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

const WIDTH: usize = 1024;
const HEIGHT: usize = 768;

/// Represents the state of the volatility surface visualization
pub struct VolatilitySurfaceVisualizer {
    window: Window,
    buffer: Vec<u32>,
    surface: Arc<Mutex<Option<VolatilitySurface>>>,
    symbol: String,
    last_update: Instant,
}

impl VolatilitySurfaceVisualizer {
    /// Create a new volatility surface visualizer
    pub fn new(symbol: &str) -> Result<Self> {
        // Create a window
        let mut window = Window::new(
            &format!("{} - Options Volatility Surface", symbol),
            WIDTH,
            HEIGHT,
            WindowOptions::default(),
        )
        .map_err(|e| OptionsError::Other(format!("Error creating window: {}", e)))?;

        // Limit to max ~60 fps
        window.limit_update_rate(Some(Duration::from_micros(16600)));

        // Create buffer for our pixels
        let buffer = vec![0; WIDTH * HEIGHT];

        Ok(Self {
            window,
            buffer,
            surface: Arc::new(Mutex::new(None)),
            symbol: symbol.to_string(),
            last_update: Instant::now(),
        })
    }

    /// Start the visualization loop
    pub fn run(&mut self, alpaca_config: crate::config::AlpacaConfig) -> Result<()> {
        // Create a channel for sending surface updates
        let (tx, rx) = std::sync::mpsc::channel();

        // Clone the surface for the data fetching thread
        let surface_clone = Arc::clone(&self.surface);
        let symbol_clone = self.symbol.clone();

        // Spawn a thread to fetch data and update the surface
        thread::spawn(move || {
            // Create a tokio runtime for async calls
            let rt = Runtime::new().unwrap();

            // Create a REST client
            let rest_client = RestClient::new(alpaca_config);

            // Fetch data and update the surface periodically
            loop {
                // Fetch option data
                let data_points =
                    rt.block_on(async { fetch_option_data(&rest_client, &symbol_clone).await });

                match data_points {
                    Ok(data) => {
                        if !data.is_empty() {
                            // Create or update the volatility surface
                            let mut surface_guard = surface_clone.lock().unwrap();

                            // Convert data points to ImpliedVolatility objects
                            let mut ivs = Vec::new();
                            for (strike, expiry, iv) in &data {
                                // Convert expiry from years to a DateTime
                                let now = Utc::now();
                                let expiration = now
                                    + chrono::Duration::seconds(
                                        (expiry * 365.0 * 24.0 * 60.0 * 60.0) as i64,
                                    );

                                // Create an OptionContract
                                let contract = OptionContract::new(
                                    symbol_clone.clone(),
                                    OptionType::Call, // Default to Call
                                    *strike,
                                    expiration,
                                );

                                // Create an ImpliedVolatility object with the correct fields
                                let iv_obj = ImpliedVolatility {
                                    contract,
                                    value: *iv,
                                    underlying_price: 0.0, // Placeholder
                                    option_price: 0.0,     // Placeholder
                                    time_to_expiration: *expiry,
                                    delta: 0.0, // Placeholder
                                    vega: 0.0,  // Placeholder
                                };

                                ivs.push(iv_obj);
                            }

                            // Create or update the surface
                            if surface_guard.is_none() {
                                match VolatilitySurface::new(symbol_clone.clone(), &ivs) {
                                    Ok(new_surface) => {
                                        *surface_guard = Some(new_surface);
                                        println!(
                                            "Created new volatility surface with {} data points",
                                            ivs.len()
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!("Error creating volatility surface: {}", e);
                                    }
                                }
                            } else if let Some(ref mut surface) = *surface_guard {
                                match surface.update(&ivs) {
                                    Ok(updated) => {
                                        if updated {
                                            println!(
                                                "Updated volatility surface with {} data points",
                                                ivs.len()
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Error updating volatility surface: {}", e);
                                    }
                                }
                            }

                            // Notify the main thread that we have new data
                            let _ = tx.send(());
                        }
                    }
                    Err(e) => {
                        eprintln!("Error fetching option data: {}", e);
                    }
                }

                // Sleep for a bit before fetching again
                thread::sleep(Duration::from_secs(5));
            }
        });

        // Main render loop
        while self.window.is_open() && !self.window.is_key_down(Key::Escape) {
            // Check for data updates
            if rx.try_recv().is_ok() || self.last_update.elapsed() > Duration::from_secs(1) {
                self.render_surface()?;
                self.last_update = Instant::now();
            }

            // Update the window with our pixel buffer
            self.window
                .update_with_buffer(&self.buffer, WIDTH, HEIGHT)
                .map_err(|e| OptionsError::Other(format!("Error updating window: {}", e)))?;
        }

        Ok(())
    }

    /// Render the volatility surface to the buffer
    fn render_surface(&mut self) -> Result<()> {
        // Convert u32 buffer to u8 buffer for plotters
        let mut u8_buffer = vec![0u8; WIDTH * HEIGHT * 4];

        {
            // Create a drawing area from our pixel buffer
            let root = BitMapBackend::with_buffer(&mut u8_buffer, (WIDTH as u32, HEIGHT as u32))
                .into_drawing_area();

            // Clear the drawing area
            root.fill(&WHITE)?;

            // Get the surface data
            let surface_guard = self.surface.lock().unwrap();

            if let Some(ref surface) = *surface_guard {
                // Draw the volatility surface
                draw_volatility_surface_heatmap(&root, surface)?;
            } else {
                // Draw a message if no data is available
                let text_style = TextStyle::from(("sans-serif", 30).into_font()).color(&BLACK);
                root.draw_text(
                    &format!("Waiting for data for {}", self.symbol),
                    &text_style,
                    (WIDTH as i32 / 2 - 200, HEIGHT as i32 / 2),
                )?;
            }

            // Finish drawing
            root.present()?;
        } // root is dropped here, releasing the borrow on u8_buffer

        // Convert u8 buffer back to u32 buffer for minifb
        for i in 0..WIDTH * HEIGHT {
            let r = u8_buffer[i * 4] as u32;
            let g = u8_buffer[i * 4 + 1] as u32;
            let b = u8_buffer[i * 4 + 2] as u32;
            let a = u8_buffer[i * 4 + 3] as u32;

            // ARGB format for minifb
            self.buffer[i] = (a << 24) | (r << 16) | (g << 8) | b;
        }

        Ok(())
    }
}

/// Fetch option data from the Alpaca API
async fn fetch_option_data(
    rest_client: &RestClient,
    underlying_symbol: &str,
) -> Result<Vec<(f64, f64, f64)>> {
    // Fetch option chain snapshots
    let snapshots = rest_client
        .get_option_chain_snapshots(
            underlying_symbol,
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

    // Convert to (strike, expiration_date_as_float, implied_volatility) tuples
    let mut data_points = Vec::new();

    for (symbol_key, snapshot) in snapshots.snapshots {
        // Try to create a contract from the OCC symbol
        if let Some(contract) = OptionContract::from_occ_symbol(&symbol_key) {
            // Extract quote data
            let mut bid: Option<f64> = None;
            let mut ask: Option<f64> = None;

            // Try to get data from last_quote
            if let Some(quote) = &snapshot.last_quote {
                bid = Some(quote.bid);
                ask = Some(quote.ask);
            }

            // If we have bid and ask, calculate implied volatility
            if let (Some(_bid), Some(_ask)) = (bid, ask) {
                // Calculate days to expiry
                let now = Utc::now();
                let days_to_expiry =
                    (contract.expiration - now).num_seconds() as f64 / (24.0 * 60.0 * 60.0);
                let years_to_expiry = days_to_expiry / 365.0;

                // Use greeks if available, otherwise use a placeholder
                let iv = if let Some(greeks) = &snapshot.greeks {
                    // Use vega as a proxy for implied volatility
                    // In a real implementation, you'd calculate IV from option prices
                    greeks.vega
                } else {
                    // Placeholder - in a real implementation, you'd calculate IV
                    0.2
                };

                data_points.push((contract.strike, years_to_expiry, iv));
            }
        }
    }

    Ok(data_points)
}

/// Draw a heatmap of the volatility surface
fn draw_volatility_surface_heatmap(
    root: &DrawingArea<BitMapBackend, Shift>,
    surface: &VolatilitySurface,
) -> Result<()> {
    let now = Utc::now();
    let times_to_expiration: Vec<f64> = surface
        .expirations
        .iter()
        .map(|&exp| {
            if exp <= now {
                0.0
            } else {
                (exp - now).num_seconds() as f64 / (365.0 * 24.0 * 60.0 * 60.0)
            }
        })
        .collect();

    let min_strike = surface.strikes.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_strike = surface
        .strikes
        .iter()
        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let min_time = times_to_expiration
        .iter()
        .fold(f64::INFINITY, |a, &b| a.min(b));
    let max_time = times_to_expiration
        .iter()
        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));

    let mut min_vol = f64::INFINITY;
    let mut max_vol = f64::NEG_INFINITY;
    for &vol in surface.volatilities.iter() {
        if !vol.is_nan() {
            min_vol = min_vol.min(vol);
            max_vol = max_vol.max(vol);
        }
    }

    let strike_range = max_strike - min_strike;
    let time_range = max_time - min_time;
    let vol_range = max_vol - min_vol;
    let strike_min = min_strike - 0.05 * strike_range;
    let strike_max = max_strike + 0.05 * strike_range;
    let time_min = min_time.max(0.0);
    let time_max = max_time + 0.05 * time_range;
    let vol_min = (min_vol - 0.1 * vol_range).max(0.0);
    let vol_max = max_vol + 0.1 * vol_range;

    let mut chart = ChartBuilder::on(root)
        .caption(
            format!("{} Volatility Surface", surface.symbol),
            ("sans-serif", 30).into_font(),
        )
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(strike_min..strike_max, time_min..time_max)?;

    chart
        .configure_mesh()
        .x_desc("Strike Price")
        .y_desc("Time to Expiration (Years)")
        .axis_desc_style(("sans-serif", 15))
        .draw()?;

    let color_gradient = colorous::VIRIDIS;

    for (i, &time) in times_to_expiration.iter().enumerate() {
        for (j, &strike) in surface.strikes.iter().enumerate() {
            let vol = surface.volatilities[[i, j]];
            if !vol.is_nan() {
                let normalized_vol = (vol - vol_min) / (vol_max - vol_min);
                let color = color_gradient.eval_continuous(normalized_vol);
                let rgb = RGBColor(color.r, color.g, color.b);

                chart.draw_series(std::iter::once(Rectangle::new(
                    [
                        (
                            strike - 0.5 * strike_range / surface.strikes.len() as f64,
                            time - 0.5 * time_range / times_to_expiration.len() as f64,
                        ),
                        (
                            strike + 0.5 * strike_range / surface.strikes.len() as f64,
                            time + 0.5 * time_range / times_to_expiration.len() as f64,
                        ),
                    ],
                    rgb.filled(),
                )))?;
            }
        }
    }

    // Draw color bar
    let color_bar_width = 20;
    let color_bar_height = 400;
    let color_bar_x = WIDTH as i32 - 100;
    let color_bar_y = 100;

    for i in 0..color_bar_height {
        let normalized_pos = 1.0 - (i as f64 / color_bar_height as f64);
        let color = color_gradient.eval_continuous(normalized_pos);
        let rgb = RGBColor(color.r, color.g, color.b);

        root.draw(&Rectangle::new(
            [
                (color_bar_x, color_bar_y + i),
                (color_bar_x + color_bar_width, color_bar_y + i + 1),
            ],
            rgb.filled(),
        ))?;
    }

    // Draw color bar labels
    root.draw_text(
        &format!("{:.2}", vol_max),
        &TextStyle::from(("sans-serif", 12)).color(&BLACK),
        (color_bar_x + color_bar_width + 5, color_bar_y),
    )?;

    root.draw_text(
        &format!("{:.2}", vol_min),
        &TextStyle::from(("sans-serif", 12)).color(&BLACK),
        (
            color_bar_x + color_bar_width + 5,
            color_bar_y + color_bar_height,
        ),
    )?;

    // Draw timestamp
    root.draw_text(
        &format!("Generated: {}", Utc::now().format("%Y-%m-%d %H:%M:%S UTC")),
        &TextStyle::from(("sans-serif", 15)).color(&BLACK),
        (10, HEIGHT as i32 - 30),
    )?;

    Ok(())
}
