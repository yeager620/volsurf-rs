use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub equity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub class: String,
    pub symbol: String,
    pub name: String,
}

/// REST client for Alpaca Markets API
pub struct RestClient {
    client: reqwest::Client,
    config: AlpacaConfig,
}

impl RestClient {
    /// Create a new REST client
    pub fn new(config: AlpacaConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    /// Helper to attach authentication headers
    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req
            .header("APCA-API-KEY-ID", &self.config.api_key)
            .header("APCA-API-SECRET-KEY", &self.config.api_secret)
    }

    /// Get account information
    pub async fn get_account(&self) -> Result<Account> {
        debug!("Getting account information");
        let url = format!("{}/v2/account", self.config.base_url);
        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Request failed: {}", e)))?;
        let acc = resp
            .json::<Account>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse account: {}", e)))?;
        Ok(acc)
    }

    /// Get assets
    pub async fn get_assets(&self, asset_class: Option<&str>) -> Result<Vec<Asset>> {
        debug!("Getting assets");
        let mut url = format!("{}/v2/assets", self.config.base_url);
        if let Some(class) = asset_class {
            url.push_str(&format!("?asset_class={}", class));
        }
        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Request failed: {}", e)))?;
        let assets = resp
            .json::<Vec<Asset>>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse assets: {}", e)))?;
        Ok(assets)
    }

    /// Get option contracts for an underlying symbol
    pub async fn get_options_chain(&self, symbol: &str, expiration_date: Option<&str>) -> Result<serde_json::Value> {
        info!("Getting option contracts for {}", symbol);
        let mut url = format!("{}/v2/options/contracts?underlying_symbols={}", self.config.data_url, symbol);

        if let Some(date) = expiration_date {
            url.push_str(&format!("&expiration_date_lte={}", date));
        }

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get options chain: {}", e)))?;

        let data = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse options chain: {}", e)))?;

        Ok(data)
    }

    /// Get historical options data
    pub async fn get_options_bars(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
        limit: Option<u32>,
        page_token: Option<&str>,
        sort: Option<&str>,
    ) -> Result<serde_json::Value> {
        debug!("Getting options bars for {} from {} to {}", symbol, start, end);
        let mut url = format!(
            "{}/v1beta1/options/bars?symbols={}&start={}&end={}&timeframe={}",
            self.config.data_url,
            symbol,
            start.to_rfc3339(),
            end.to_rfc3339(),
            timeframe
        );

        if let Some(limit_val) = limit {
            url.push_str(&format!("&limit={}", limit_val));
        }

        if let Some(token) = page_token {
            url.push_str(&format!("&page_token={}", token));
        }

        if let Some(sort_order) = sort {
            url.push_str(&format!("&sort={}", sort_order));
        }

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get options bars: {}", e)))?;

        let data = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse options bars: {}", e)))?;

        Ok(data)
    }

    /// Get options trades
    pub async fn get_options_trades(
        &self,
        symbols: &[&str],
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        page_token: Option<&str>,
        sort: Option<&str>,
    ) -> Result<serde_json::Value> {
        debug!("Getting options trades for symbols: {:?}", symbols);
        let symbols_str = symbols.join(",");
        let mut url = format!(
            "{}/v1beta1/options/trades?symbols={}",
            self.config.data_url, symbols_str
        );

        if let Some(start_time) = start {
            url.push_str(&format!("&start={}", start_time.to_rfc3339()));
        }

        if let Some(end_time) = end {
            url.push_str(&format!("&end={}", end_time.to_rfc3339()));
        }

        if let Some(limit_val) = limit {
            url.push_str(&format!("&limit={}", limit_val));
        }

        if let Some(token) = page_token {
            url.push_str(&format!("&page_token={}", token));
        }

        if let Some(sort_order) = sort {
            url.push_str(&format!("&sort={}", sort_order));
        }

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get options trades: {}", e)))?;

        let data = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse options trades: {}", e)))?;

        Ok(data)
    }

    /// Get latest options quotes
    pub async fn get_options_quotes(
        &self,
        symbols: &[&str],
    ) -> Result<serde_json::Value> {
        debug!("Getting latest options quotes for symbols: {:?}", symbols);
        let symbols_str = symbols.join(",");
        let url = format!(
            "{}/v1beta1/options/quotes/latest?symbols={}",
            self.config.data_url, symbols_str
        );

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get options quotes: {}", e)))?;

        let data = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse options quotes: {}", e)))?;

        Ok(data)
    }
}
