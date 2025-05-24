use crate::error::{OptionsError, Result};
use dotenv::dotenv;
use serde::Deserialize;
use std::env;

/// Configuration for the Alpaca API
#[derive(Debug, Clone, Deserialize)]
pub struct AlpacaConfig {
    /// Alpaca API key
    pub api_key: String,
    /// Alpaca API secret
    pub api_secret: String,
    /// Alpaca API base URL
    pub base_url: String,
    /// Alpaca API data URL
    pub data_url: String,
}

/// Configuration for the application
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Alpaca API configuration
    pub alpaca: AlpacaConfig,
    /// Log level
    pub log_level: String,
    /// Whether to use paper trading
    pub paper_trading: bool,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        // Load .env file if it exists
        dotenv().ok();

        // Default values
        let default_log_level = "info".to_string();
        let default_paper_trading = true;
        let default_base_url = if default_paper_trading {
            "https://paper-api.alpaca.markets".to_string()
        } else {
            "https://api.alpaca.markets".to_string()
        };
        let default_data_url = "https://data.alpaca.markets".to_string();

        // Get API key and secret from environment variables
        let api_key = env::var("ALPACA_API_KEY").map_err(|_| {
            OptionsError::ConfigError("ALPACA_API_KEY environment variable not set".to_string())
        })?;

        let api_secret = env::var("ALPACA_API_SECRET").map_err(|_| {
            OptionsError::ConfigError("ALPACA_API_SECRET environment variable not set".to_string())
        })?;

        // Get other configuration values from environment variables or use defaults
        let base_url = env::var("ALPACA_BASE_URL").unwrap_or(default_base_url);
        let data_url = env::var("ALPACA_DATA_URL").unwrap_or(default_data_url);
        let log_level = env::var("LOG_LEVEL").unwrap_or(default_log_level);
        let paper_trading = env::var("PAPER_TRADING")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(default_paper_trading);

        Ok(Config {
            alpaca: AlpacaConfig {
                api_key,
                api_secret,
                base_url,
                data_url,
            },
            log_level,
            paper_trading,
        })
    }

    /// Initialize logging based on configuration
    pub fn init_logging(&self) -> Result<()> {
        use tracing_subscriber::{fmt, EnvFilter};

        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(&self.log_level));

        fmt()
            .with_env_filter(filter)
            .with_target(true)
            .init();

        Ok(())
    }
}