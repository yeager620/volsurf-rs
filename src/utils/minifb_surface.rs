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
            if let Some(ref surf) = self.latest {
                self.draw_heatmap(surf)?;
            }
            self.window
                .update_with_buffer(&self.buffer, self.width, self.height)
                .map_err(|e| OptionsError::Other(format!("Window update: {}", e)))?;
        }
        Ok(())
    }

    fn draw_heatmap(&mut self, surf: &SurfaceUpdate) -> Result<()> {
        let root = BitMapBackend::with_buffer(
            &mut self.buffer,
            (self.width as u32, self.height as u32),
        ).into_drawing_area();
        root.fill(&BLACK)?;
        let z = ndarray::Array2::from_shape_vec(
            (surf.expiries.len(), surf.strikes.len()),
            surf.sigma.clone(),
        ).map_err(|e| OptionsError::Other(format!("array shape: {}", e)))?;
        root.plot(&plotters::element::HeatMap::new(
            z.indexed_iter().map(|((row, col), &sigma)| {
                ((surf.strikes[col], row as f64), sigma)
            }),
            plotters::style::Palette99,
        ))?;
        root.present()?;
        Ok(())
    }
}

/// Helper struct accumulating quotes into a surface grid
struct SurfaceBuilder {
    grid: HashMap<(f64, NaiveDate), f64>,
    last_publish: Instant,
}

impl SurfaceBuilder {
    fn new() -> Self {
        Self { grid: HashMap::new(), last_publish: Instant::now() }
    }

    fn on_quote(&mut self, q: OptionQuote) -> Result<Option<SurfaceUpdate>> {
        let iv = ImpliedVolatility::from_quote(&q, 0.03)?.value;
        let key = (q.contract.strike, q.contract.expiration.date_naive());
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
        let mut strikes: Vec<f64> = self.grid.keys().map(|(k, _)| *k).collect();
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        strikes.dedup();
        let mut expiries: Vec<NaiveDate> = self.grid.keys().map(|(_, e)| *e).collect();
        expiries.sort();
        expiries.dedup();
        let mut sigma = Vec::with_capacity(expiries.len() * strikes.len());
        for exp in &expiries {
            for strike in &strikes {
                sigma.push(*self.grid.get(&(*strike, *exp)).unwrap_or(&f64::NAN));
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
            None,
            None,
            None,
            None,
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
