//! # options-rs
//!
//! A low-latency Alpaca Markets API client focused on real-time options pricing
//! and volatility surface calculations.
//!
//! ## Features
//!
//! - Async WebSocket and REST clients for Alpaca Markets API
//! - Real-time options data streaming
//! - Implied volatility and volatility surface calculations
//! - Visualization tools for volatility surfaces
//! - Environment-based configuration
//!
//! ## Example
//!
//! ```rust,no_run
//! use options_rs::api::{RestClient, WebSocketClient};
//! use options_rs::config::Config;
//! use options_rs::models::volatility::ImpliedVolatility;
//! use options_rs::utils::plot_volatility_surface;
//! use std::collections::HashMap;
//! use tokio;
//!
//! #[tokio::main]
//! async fn main() -> options_rs::error::Result<()> {
//!     // Load configuration from environment
//!     let config = Config::from_env()?;
//!     config.init_logging()?;
//!
//!     // Create API clients
//!     let rest_client = RestClient::new(config.alpaca.clone());
//!     let ws_client = WebSocketClient::new(config.alpaca.clone());
//!
//!     // Connect to WebSocket and subscribe to options for a symbol
//!     let symbol = "AAPL";
//!     let options = rest_client.get_options(symbol).await?;
//!     let option_symbols: Vec<String> = options.iter()
//!         .map(|opt| opt.symbol.clone())
//!         .collect();
//!
//!     ws_client.connect(option_symbols).await?;
//!
//!     // Process option quotes and calculate implied volatility
//!     let mut ivs = Vec::new();
//!     let risk_free_rate = 0.03; // 3% risk-free rate
//!
//!     for _ in 0..10 {
//!         if let Some(quote) = ws_client.next_option_quote().await? {
//!             if let Ok(iv) = ImpliedVolatility::from_quote(&quote, risk_free_rate) {
//!                 ivs.push(iv);
//!             }
//!         }
//!     }
//!
//!     // Create and plot volatility surface
//!     let surface = options_rs::models::volatility::VolatilitySurface::new(
//!         symbol.to_string(),
//!         &ivs,
//!     )?;
//!
//!     plot_volatility_surface(&surface, "volatility_surface.png")?;
//!
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod config;
pub mod error;
pub mod models;
pub mod utils;

// Re-export commonly used types
pub use api::{RestClient, WebSocketClient};
pub use config::Config;
pub use error::{OptionsError, Result};
