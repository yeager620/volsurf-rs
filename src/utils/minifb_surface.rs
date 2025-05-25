use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use once_cell::sync::Lazy;
use tokio::sync::broadcast;

use crate::api::{RestClient, WebSocketClient};
use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use crate::models::{ImpliedVolatility, OptionQuote, SurfaceUpdate};

use minifb::{Key, Window, WindowOptions};
use plotters::prelude::*;

use ndarray;
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
}

impl VolatilitySurfaceVisualizer {
    pub fn new(symbol: &str) -> Result<Self> {
        let (width, height) = (1024, 768);
        let mut window = Window::new(
            &format!("IV Surface – {}", symbol),
            width,
            height,
            WindowOptions::default(),
        ).map_err(|e| OptionsError::Other(format!("Window error: {}", e)))?;
        window.limit_update_rate(Some(Duration::from_micros(1_000_000 / 60)));
        let buffer = vec![0u32; width * height];
        let rx = SURFACE_BUS.subscribe();
        Ok(Self { window, buffer, width, height, rx, latest: None })
    }

    pub fn run(&mut self, _cfg: AlpacaConfig) -> Result<()> {
        while self.window.is_open() && !self.window.is_key_down(Key::Escape) {
            while let Ok(update) = self.rx.try_recv() {
                self.latest = Some(update);
            }

            if let Some(update) = self.latest.clone() {
                self.draw_heatmap(&update)?;
            }
            self.window
                .update_with_buffer(&self.buffer, self.width, self.height)
                .map_err(|e| OptionsError::Other(format!("Window update: {}", e)))?;
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
            let root = BitMapBackend::with_buffer(
                &mut u8_buffer,
                (self.width as u32, self.height as u32),
            )
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
                let idx = (norm.clamp(0.0, 1.0) * (Palette99::COLORS.len() - 1) as f64)
                    .round() as usize;
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
            let r = u8_buffer[i * 4] as u32;
            let g = u8_buffer[i * 4 + 1] as u32;
            let b = u8_buffer[i * 4 + 2] as u32;
            let a = u8_buffer[i * 4 + 3] as u32;
            self.buffer[i] = (a << 24) | (r << 16) | (g << 8) | b;
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
        Self { grid: HashMap::new(), last_publish: Instant::now() }
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
        let mut strikes: Vec<f64> = self
            .grid
            .keys()
            .map(|(k, _)| (*k as f64) / 100.0)
            .collect();
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
        SurfaceUpdate { strikes, expiries, sigma }
    }
}

pub async fn stream_quotes(symbol: String, cfg: AlpacaConfig) -> Result<()> {
    let rest = RestClient::new(cfg.clone());
    let contracts = rest
        .get_options_chain(
            &symbol,
            None,
            None,
            None,
            None,
            None,
            Some(1000),
            None,
        )
        .await?;
    let option_symbols: Vec<String> = contracts.results.iter().map(|c| c.symbol.clone()).collect();
    if option_symbols.is_empty() {
        return Ok(());
    }
    let ws = WebSocketClient::new(cfg);
    ws.connect(option_symbols).await?;
    let mut builder = SurfaceBuilder::new();
    while let Some(q) = ws.next_option_quote().await? {
        if let Some(update) = builder.on_quote(q)? {
            let _ = SURFACE_BUS.send(update);
        }
    }
    Ok(())
}
