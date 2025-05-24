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

pub struct RestClient {
    client: reqwest::Client,
    config: AlpacaConfig,
}

impl RestClient {
    pub fn new(config: AlpacaConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("APCA-API-KEY-ID", &self.config.api_key)
            .header("APCA-API-SECRET-KEY", &self.config.api_secret)
    }

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

    /// get option contracts for an underlying symbol
    pub async fn get_options_chain(
        &self,
        symbol: &str,
        expiration_date: Option<&str>,
    ) -> Result<serde_json::Value> {
        info!("Getting option contracts for {}", symbol);
        let mut url = format!(
            "{}/v2/options/contracts?underlying_symbols={}",
            self.config.data_url, symbol
        );

        if let Some(date) = expiration_date {
            url.push_str(&format!("&expiration_date_lte={}", date));
        }

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get options chain: {}", e)))?;

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options chain: {}", e))
        })?;

        Ok(data)
    }

    /// get historical options data
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
        debug!(
            "Getting options bars for {} from {} to {}",
            symbol, start, end
        );
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

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options bars: {}", e))
        })?;

        Ok(data)
    }

    /// get options trades
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

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options trades: {}", e))
        })?;

        Ok(data)
    }

    /// get latest options quotes
    pub async fn get_options_quotes(&self, symbols: &[&str]) -> Result<serde_json::Value> {
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

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options quotes: {}", e))
        })?;

        Ok(data)
    }

    /// get snapshots for a list of option symbols
    pub async fn get_option_snapshots(
        &self,
        symbols: &[&str],
        feed: Option<&str>,
        updated_since: Option<DateTime<Utc>>,
        limit: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<serde_json::Value> {
        let symbols_str = symbols.join(",");
        let mut url = format!(
            "{}/v1beta1/options/snapshots?symbols={}",
            self.config.data_url, symbols_str
        );

        if let Some(feed_val) = feed {
            url.push_str(&format!("&feed={}", feed_val));
        }

        if let Some(updated) = updated_since {
            url.push_str(&format!("&updated_since={}", updated.to_rfc3339()));
        }

        if let Some(limit_val) = limit {
            url.push_str(&format!("&limit={}", limit_val));
        }

        if let Some(token) = page_token {
            url.push_str(&format!("&page_token={}", token));
        }

        let resp =
            self.auth(self.client.get(&url)).send().await.map_err(|e| {
                OptionsError::Other(format!("Failed to get option snapshots: {}", e))
            })?;

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse option snapshots: {}", e))
        })?;

        Ok(data)
    }

    /// get option chain snapshots for an underlying symbol
    #[allow(clippy::too_many_arguments)]
    pub async fn get_option_chain_snapshots(
        &self,
        underlying_symbol: &str,
        feed: Option<&str>,
        limit: Option<u32>,
        updated_since: Option<DateTime<Utc>>,
        page_token: Option<&str>,
        option_type: Option<&str>,
        strike_price_gte: Option<f64>,
        strike_price_lte: Option<f64>,
        expiration_date: Option<&str>,
        expiration_date_gte: Option<&str>,
        expiration_date_lte: Option<&str>,
        root_symbol: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut url = format!(
            "{}/v1beta1/options/snapshots/{}",
            self.config.data_url, underlying_symbol
        );

        let mut query_params = Vec::new();
        if let Some(feed_val) = feed {
            query_params.push(format!("feed={}", feed_val));
        }
        if let Some(limit_val) = limit {
            query_params.push(format!("limit={}", limit_val));
        }
        if let Some(updated) = updated_since {
            query_params.push(format!("updated_since={}", updated.to_rfc3339()));
        }
        if let Some(token) = page_token {
            query_params.push(format!("page_token={}", token));
        }
        if let Some(t) = option_type {
            query_params.push(format!("type={}", t));
        }
        if let Some(v) = strike_price_gte {
            query_params.push(format!("strike_price_gte={}", v));
        }
        if let Some(v) = strike_price_lte {
            query_params.push(format!("strike_price_lte={}", v));
        }
        if let Some(v) = expiration_date {
            query_params.push(format!("expiration_date={}", v));
        }
        if let Some(v) = expiration_date_gte {
            query_params.push(format!("expiration_date_gte={}", v));
        }
        if let Some(v) = expiration_date_lte {
            query_params.push(format!("expiration_date_lte={}", v));
        }
        if let Some(v) = root_symbol {
            query_params.push(format!("root_symbol={}", v));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

        let resp = self.auth(self.client.get(&url)).send().await.map_err(|e| {
            OptionsError::Other(format!("Failed to get option chain snapshots: {}", e))
        })?;

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse option chain snapshots: {}", e))
        })?;

        Ok(data)
    }
}
