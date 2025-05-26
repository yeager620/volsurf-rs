use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDate, Utc};
use once_cell::sync::Lazy;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::api::RestClient;
use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use crate::models::{ImpliedVolatility, OptionContract, OptionQuote, SurfaceUpdate};

use minifb::{Key, Window, WindowOptions};
use plotters::prelude::*;

/// Global broadcast channel for surface updates
pub static SURFACE_BUS: Lazy<broadcast::Sender<SurfaceUpdate>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(32);
    tx
});

/// Visualizes volatility surface updates using MiniFB
pub struct VolatilitySurfaceVisualizer {
    window: Window,
    buffer: Vec<u32>,
    width: usize,
    height: usize,
    rx: broadcast::Receiver<SurfaceUpdate>,
    latest: Option<SurfaceUpdate>,
    last_update_time: std::time::Instant,
}

impl VolatilitySurfaceVisualizer {
    pub fn new(symbol: &str) -> Result<Self> {
        let (width, height) = (1024, 768);
        let mut window = Window::new(
            &format!("IV Surface – {}", symbol),
            width,
            height,
            WindowOptions::default(),
        )
        .map_err(|e| OptionsError::Other(format!("Window error: {}", e)))?;
        window.limit_update_rate(Some(Duration::from_micros(1_000_000 / 60)));
        let buffer = vec![0u32; width * height];
        let rx = SURFACE_BUS.subscribe();
        Ok(Self {
            window,
            buffer,
            width,
            height,
            rx,
            latest: None,
            last_update_time: Instant::now(),
        })
    }

    pub fn run(&mut self, _cfg: AlpacaConfig) -> Result<()> {
        let start_time = Instant::now();

        while self.window.is_open() && !self.window.is_key_down(Key::Escape) {
            while let Ok(update) = self.rx.try_recv() {
                self.latest = Some(update);
            }

            if let Some(update) = self.latest.clone() {
                self.draw_heatmap(&update)?;
            } else {
                // If no data is available yet, draw a loading message
                self.draw_loading_message(start_time.elapsed().as_secs())?;
            }

            self.window
                .update_with_buffer(&self.buffer, self.width, self.height)
                .map_err(|e| OptionsError::Other(format!("Window update: {}", e)))?;
        }
        Ok(())
    }

    fn draw_loading_message(&mut self, elapsed_secs: u64) -> Result<()> {
        use plotters::style::TextStyle;

        // Clear buffer with black
        for pixel in self.buffer.iter_mut() {
            *pixel = 0;
        }

        let mut u8_buffer = vec![0u8; self.width * self.height * 4];

        {
            let root =
                BitMapBackend::with_buffer(&mut u8_buffer, (self.width as u32, self.height as u32))
                    .into_drawing_area();

            root.fill(&BLACK)?;

            // Draw loading message
            let loading_text = format!("Loading data... ({} seconds)", elapsed_secs);
            let style = TextStyle::from(("sans-serif", 30).into_font()).color(&WHITE);

            root.draw_text(
                &loading_text,
                &style,
                (self.width as i32 / 2, self.height as i32 / 2),
            )?;

            // Add a hint about data loading
            let hint_text = "Fetching option contracts and establishing WebSocket connection...";
            let hint_style = TextStyle::from(("sans-serif", 20).into_font()).color(&WHITE);

            root.draw_text(
                hint_text,
                &hint_style,
                (self.width as i32 / 2, self.height as i32 / 2 + 40),
            )?;

            root.present()?;
        }

        // Convert the u8 buffer to u32 buffer for minifb
        for i in 0..self.width * self.height {
            // Plotters writes into u8_buffer as [R, G, B, A]
            let r = u8_buffer[i * 4] as u32;
            let g = u8_buffer[i * 4 + 1] as u32;
            let b = u8_buffer[i * 4 + 2] as u32;
            // Force fully opaque alpha
            let a = 0xff;

            // MiniFB on macOS expects ARGB in memory as [B, G, R, A] little-endian,
            // so pack as BGRA:
            self.buffer[i] = (a << 24) | (b << 16) | (g << 8) | r;
        }

        Ok(())
    }

    fn draw_heatmap(&mut self, surf: &SurfaceUpdate) -> Result<()> {
        use plotters::style::Palette;

        let mut u8_buffer = vec![0u8; self.width * self.height * 4];

        if surf.strikes.is_empty() || surf.expiries.is_empty() {
            return Ok(());
        }

        {
            let root =
                BitMapBackend::with_buffer(&mut u8_buffer, (self.width as u32, self.height as u32))
                    .into_drawing_area();

            root.fill(&BLACK)?;

            let strike_step = if surf.strikes.len() > 1 {
                surf.strikes[1] - surf.strikes[0]
            } else {
                1.0
            };

            let min_vol = surf
                .sigma
                .iter()
                .cloned()
                .filter(|v| v.is_finite())
                .fold(f64::INFINITY, f64::min);
            let max_vol = surf
                .sigma
                .iter()
                .cloned()
                .filter(|v| v.is_finite())
                .fold(f64::NEG_INFINITY, f64::max);

            let mut chart = ChartBuilder::on(&root)
                .margin(10)
                .x_label_area_size(40)
                .y_label_area_size(40)
                .build_cartesian_2d(
                    *surf.strikes.first().unwrap()..*surf.strikes.last().unwrap(),
                    0f64..surf.expiries.len() as f64,
                )?;

            chart.configure_mesh().disable_mesh().draw()?;

            for (row, _exp) in surf.expiries.iter().enumerate() {
                for (col, &strike) in surf.strikes.iter().enumerate() {
                    let sigma = surf.sigma[row * surf.strikes.len() + col];
                    if !sigma.is_finite() {
                        continue;
                    }

                    let norm = if max_vol > min_vol {
                        (sigma - min_vol) / (max_vol - min_vol)
                    } else {
                        0.0
                    };
                    let idx = (norm.clamp(0.0, 1.0) * (Palette99::COLORS.len() - 1) as f64).round()
                        as usize;
                    let color = Palette99::pick(idx);
                    let rect = Rectangle::new(
                        [
                            (strike - 0.5 * strike_step, row as f64),
                            (strike + 0.5 * strike_step, row as f64 + 1.0),
                        ],
                        color.filled(),
                    );
                    chart.draw_series(std::iter::once(rect))?;
                }
            }

            root.present()?;
        }

        for i in 0..self.width * self.height {
            // Plotters writes into u8_buffer as [R, G, B, A]
            let r = u8_buffer[i * 4] as u32;
            let g = u8_buffer[i * 4 + 1] as u32;
            let b = u8_buffer[i * 4 + 2] as u32;
            // Force fully opaque alpha
            let a = 0xff;

            // MiniFB on macOS expects ARGB in memory as [B, G, R, A] little-endian,
            // so pack as BGRA:
            self.buffer[i] = (a << 24) | (b << 16) | (g << 8) | r;
        }

        Ok(())
    }
}

/// Helper struct accumulating quotes into a surface grid
struct SurfaceBuilder {
    grid: HashMap<(i64, NaiveDate), f64>,
    last_publish: Instant,
}

impl SurfaceBuilder {
    fn new() -> Self {
        Self {
            grid: HashMap::new(),
            last_publish: Instant::now(),
        }
    }

    fn on_quote(&mut self, q: OptionQuote) -> Result<Option<SurfaceUpdate>> {
        let iv = ImpliedVolatility::from_quote(&q, 0.03)?.value;
        let strike_key = (q.contract.strike * 100.0).round() as i64;
        let key = (strike_key, q.contract.expiration.date_naive());
        self.grid.insert(key, iv);
        if self.last_publish.elapsed() >= Duration::from_millis(500) {
            let update = self.to_surface_update();
            self.last_publish = Instant::now();
            Ok(Some(update))
        } else {
            Ok(None)
        }
    }

    fn to_surface_update(&self) -> SurfaceUpdate {
        let mut strikes: Vec<f64> = self.grid.keys().map(|(k, _)| (*k as f64) / 100.0).collect();
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        strikes.dedup();
        let mut expiries: Vec<NaiveDate> = self.grid.keys().map(|(_, e)| *e).collect();
        expiries.sort();
        expiries.dedup();
        let mut sigma = Vec::with_capacity(expiries.len() * strikes.len());
        for exp in &expiries {
            for strike in &strikes {
                let key = ((strike * 100.0).round() as i64, *exp);
                sigma.push(*self.grid.get(&key).unwrap_or(&f64::NAN));
            }
        }
        SurfaceUpdate {
            strikes,
            expiries,
            sigma,
        }
    }
}

pub async fn stream_quotes(symbol: String, cfg: AlpacaConfig) -> Result<()> {
    let rest = RestClient::new(cfg.clone());

    // Fetch option contracts with a timeout using get_option_chain_snapshots with feed=indicative
    info!("Fetching option contracts for {}", symbol);
    let snapshots = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        rest.get_option_chain_snapshots(
            &symbol,
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
        ),
    )
    .await
    {
        Ok(result) => match result {
            Ok(snapshots) => snapshots,
            Err(e) => {
                warn!("Error fetching option contracts: {}", e);
                // Create a minimal surface update with a warning
                let update = SurfaceUpdate {
                    strikes: vec![100.0, 200.0, 300.0],
                    expiries: vec![chrono::Local::now().date_naive()],
                    sigma: vec![0.0; 3], // Just placeholder data
                };
                let _ = SURFACE_BUS.send(update);
                return Err(e);
            }
        },
        Err(_) => {
            warn!("Timeout fetching option contracts for {}", symbol);
            // Create a minimal surface update with a warning
            let update = SurfaceUpdate {
                strikes: vec![100.0, 200.0, 300.0],
                expiries: vec![chrono::Local::now().date_naive()],
                sigma: vec![0.0; 3], // Just placeholder data
            };
            let _ = SURFACE_BUS.send(update);
            return Err(OptionsError::Other(
                "Timeout fetching option contracts".to_string(),
            ));
        }
    };

    let option_symbols: Vec<String> = snapshots.snapshots.keys().cloned().collect();
    if option_symbols.is_empty() {
        warn!("No option symbols found for {}", symbol);
        warn!("This could be because:");
        warn!(
            "1. The symbol {} does not have any options available",
            symbol
        );
        warn!(
            "2. The symbol {} is not valid or not supported by Alpaca",
            symbol
        );
        warn!("3. There might be an issue with your Alpaca API credentials or subscription");
        warn!("4. The Alpaca API might be experiencing issues");

        // Create a surface update with a warning message
        let update = SurfaceUpdate {
            strikes: vec![100.0, 200.0, 300.0],
            expiries: vec![chrono::Local::now().date_naive()],
            sigma: vec![0.0; 3], // Just placeholder data
        };
        let _ = SURFACE_BUS.send(update);

        return Err(OptionsError::Other(format!(
            "No option symbols found for {}",
            symbol
        )));
    }

    info!(
        "Processing {} option snapshots for {}",
        option_symbols.len(),
        symbol
    );
    let mut builder = SurfaceBuilder::new();
    let mut processed_count = 0;
    let mut parse_failures = 0;
    let mut missing_data_count = 0;

    // Process each snapshot to create option quotes
    for (symbol_key, snapshot) in snapshots.snapshots.iter() {
        // Try to create a contract from the OCC symbol
        let contract_result = OptionContract::from_occ_symbol(symbol_key);

        if let Some(contract) = contract_result {
            // Extract quote data from the snapshot
            let mut bid: Option<f64> = None;
            let mut ask: Option<f64> = None;
            let mut last_price: Option<f64> = None;
            let mut timestamp: Option<DateTime<Utc>> = None;

            // Try to get data from last_quote and last_trade first
            if let Some(quote) = &snapshot.last_quote {
                bid = Some(quote.bid);
                ask = Some(quote.ask);
                timestamp = Some(quote.t);
            }

            if let Some(trade) = &snapshot.last_trade {
                last_price = Some(trade.price);
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
                } else if let Some(bar) = &snapshot.minuteBar {
                    last_price = Some(bar.c); // Use close as last price
                } else if let Some(bar) = &snapshot.prevDailyBar {
                    last_price = Some(bar.c); // Use close as last price
                }
            }

            // If we still don't have a timestamp, use current time
            if timestamp.is_none() {
                timestamp = Some(Utc::now());
            }

            if bid.is_some() && ask.is_some() && last_price.is_some() && timestamp.is_some() {
                let bid = bid.unwrap();
                let ask = ask.unwrap();
                let last_price = last_price.unwrap();
                let timestamp = timestamp.unwrap();

                // Estimate underlying price (not ideal but workable)
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
                    volume: 0,        // Not critical for IV calculation
                    open_interest: 0, // Not available in snapshots
                    underlying_price,
                    timestamp,
                };

                // Process the quote and potentially create a surface update
                if let Some(update) = builder.on_quote(quote)? {
                    let _ = SURFACE_BUS.send(update);
                }

                processed_count += 1;
            } else {
                missing_data_count += 1;
            }
        } else {
            parse_failures += 1;
        }
    }

    info!(
        "Processed {}/{} option snapshots",
        processed_count,
        option_symbols.len()
    );
    info!("OCC symbol parse failures: {}", parse_failures);
    info!("Missing quote/trade data: {}", missing_data_count);

    if processed_count == 0 {
        warn!(
            "No valid option quotes could be created from snapshots for {}.",
            symbol
        );
        return Err(OptionsError::Other(format!(
            "No valid option quotes for {}",
            symbol
        )));
    }

    // Send a final surface update
    let update = builder.to_surface_update();
    let _ = SURFACE_BUS.send(update);

    // Keep the task alive for a while to allow the GUI to display the data
    tokio::time::sleep(std::time::Duration::from_secs(300)).await;

    Ok(())
}
