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
}

#[derive(Debug, Clone, Deserialize)]
pub struct ETradeConfig {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_token: String,
    pub access_secret: String,
    #[serde(default)]
    pub sandbox: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub alpaca: AlpacaConfig,
    pub etrade: ETradeConfig,
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

        let api_key = env::var("ALPACA_API_KEY").map_err(|_| {
            OptionsError::ConfigError("ALPACA_API_KEY environment variable not set".to_string())
        })?;

        let api_secret = env::var("ALPACA_API_SECRET").map_err(|_| {
            OptionsError::ConfigError("ALPACA_API_SECRET environment variable not set".to_string())
        })?;

        let base_url = env::var("ALPACA_BASE_URL").unwrap_or(default_base_url);
        let data_url = env::var("ALPACA_DATA_URL").unwrap_or(default_data_url);
        let log_level = env::var("LOG_LEVEL").unwrap_or(default_log_level);
        let paper_trading = env::var("PAPER_TRADING")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(default_paper_trading);

        // Prefer production credentials if provided, fallback to sandbox
        let etrade_consumer_key = env::var("ETRADE_PROD_CONSUMER_KEY")
            .or_else(|_| env::var("ETRADE_SANDBOX_CONSUMER_KEY"))
            .or_else(|_| env::var("ETRADE_CONSUMER_KEY"))
            .map_err(|_| {
                OptionsError::ConfigError(
                    "ETRADE_PROD_CONSUMER_KEY or ETRADE_SANDBOX_CONSUMER_KEY environment variable not set".to_string(),
                )
            })?;
        let etrade_consumer_secret = env::var("ETRADE_PROD_CONSUMER_SECRET")
            .or_else(|_| env::var("ETRADE_SANDBOX_CONSUMER_SECRET"))
            .or_else(|_| env::var("ETRADE_CONSUMER_SECRET"))
            .map_err(|_| {
                OptionsError::ConfigError(
                    "ETRADE_PROD_CONSUMER_SECRET or ETRADE_SANDBOX_CONSUMER_SECRET environment variable not set".to_string(),
                )
            })?;
        // Make access token and secret optional, defaulting to empty strings
        let etrade_access_token = env::var("ETRADE_PROD_ACCESS_TOKEN")
            .or_else(|_| env::var("ETRADE_SANDBOX_ACCESS_TOKEN"))
            .or_else(|_| env::var("ETRADE_ACCESS_TOKEN"))
            .unwrap_or_default();
        let etrade_access_secret = env::var("ETRADE_PROD_ACCESS_SECRET")
            .or_else(|_| env::var("ETRADE_SANDBOX_ACCESS_SECRET"))
            .or_else(|_| env::var("ETRADE_ACCESS_SECRET"))
            .unwrap_or_default();

        // Determine if we should use sandbox based on environment variable or fallback logic
        // Check for explicit ETRADE_SANDBOX environment variable first
        let etrade_sandbox = env::var("ETRADE_SANDBOX")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or_else(|_| {
                // If not explicitly set, determine based on which credentials were loaded
                // If we successfully loaded ETRADE_PROD_CONSUMER_KEY, use production mode
                // Otherwise, default to sandbox mode
                !env::var("ETRADE_PROD_CONSUMER_KEY").is_ok()
            });

        Ok(Config {
            alpaca: AlpacaConfig {
                api_key,
                api_secret,
                base_url,
                data_url,
            },
            etrade: ETradeConfig {
                consumer_key: etrade_consumer_key,
                consumer_secret: etrade_consumer_secret,
                access_token: etrade_access_token,
                access_secret: etrade_access_secret,
                sandbox: etrade_sandbox,
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
