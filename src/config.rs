use crate::error::{OptionsError, Result};
use dotenv::dotenv;
use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct AlpacaConfig {
    pub api_key: String,
    pub api_secret: String,
    pub base_url: String,
    pub data_url: String,
    pub paper_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub alpaca: AlpacaConfig,
    pub log_level: String,
    pub paper_trading: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv().ok();

        let default_log_level = "info".to_string();
        let default_paper_trading = true;
        let default_base_url = if default_paper_trading {
            "https://paper-api.alpaca.markets".to_string()
        } else {
            "https://api.alpaca.markets".to_string()
        };
        let default_data_url = "https://data.alpaca.markets".to_string();
        let default_paper_url = "https://paper-api.alpaca.markets".to_string();

        let api_key = env::var("ALPACA_API_KEY").map_err(|_| {
            OptionsError::ConfigError("ALPACA_API_KEY environment variable not set".to_string())
        })?;

        let api_secret = env::var("ALPACA_API_SECRET").map_err(|_| {
            OptionsError::ConfigError("ALPACA_API_SECRET environment variable not set".to_string())
        })?;

        let base_url = env::var("ALPACA_BASE_URL").unwrap_or(default_base_url);
        let data_url = env::var("ALPACA_DATA_URL").unwrap_or(default_data_url);
        let paper_url = env::var("ALPACA_PAPER_URL").unwrap_or(default_paper_url.to_string());
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
                paper_url,
            },
            log_level,
            paper_trading,
        })
    }

    pub fn init_logging(&self) -> Result<()> {
        use tracing_subscriber::{fmt, EnvFilter};

        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&self.log_level));

        fmt().with_env_filter(filter).with_target(true).init();

        Ok(())
    }
}
