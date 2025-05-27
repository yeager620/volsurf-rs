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

// Define proper types for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionContract {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub status: String,
    pub tradable: bool,
    pub expiration_date: String,
    pub root_symbol: String,
    pub underlying_symbol: String,
    pub underlying_asset_id: String,
    #[serde(rename = "type")]
    pub contract_type: String,
    pub style: String,
    pub strike_price: String, // Note: API returns this as a string "5", not a number
    pub multiplier: String,
    pub size: String,
    pub open_interest: Option<String>,
    pub open_interest_date: Option<String>,
    pub close_price: Option<String>,
    pub close_price_date: Option<String>,
    pub ppind: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionContractsResponse {
    #[serde(rename = "option_contracts", default)]
    pub results: Vec<OptionContract>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionBar {
    pub t: DateTime<Utc>,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: u64,
    pub n: Option<u32>,
    pub vw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionBarsResponse {
    pub bars: std::collections::HashMap<String, Vec<OptionBar>>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionTrade {
    pub t: DateTime<Utc>,
    pub price: f64,
    pub size: u64,
    pub conditions: Vec<String>,
    pub exchange_code: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionTradesResponse {
    pub trades: Vec<OptionTrade>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionQuote {
    pub t: DateTime<Utc>,
    pub bid: f64,
    pub ask: f64,
    pub size_bid: u64,
    pub size_ask: u64,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionQuotesResponse {
    pub quotes: std::collections::HashMap<String, OptionQuote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockQuote {
    pub t: DateTime<Utc>,
    #[serde(alias = "bp")]
    pub bid: f64,
    #[serde(alias = "ap")]
    pub ask: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestStockQuotesResponse {
    pub quotes: std::collections::HashMap<String, StockQuote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleStockQuoteResponse {
    pub quote: StockQuote,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionLastTrade {
    pub t: DateTime<Utc>,
    #[serde(alias = "p")]
    pub price: f64,
    #[serde(alias = "s")]
    pub size: u64,
    #[serde(default)]
    pub conditions: Vec<String>,
    #[serde(alias = "x")]
    pub exchange_code: String,
    #[serde(alias = "c")]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionLastQuote {
    pub t: DateTime<Utc>,
    #[serde(alias = "bp")]
    pub bid: f64,
    #[serde(alias = "ap")]
    pub ask: f64,
    #[serde(alias = "bs")]
    pub size_bid: u64,
    #[serde(alias = "as")]
    pub size_ask: u64,
    #[serde(alias = "bx")]
    pub bid_exchange: Option<String>,
    #[serde(alias = "ax")]
    pub ask_exchange: Option<String>,
    #[serde(alias = "c")]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionGreeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
    pub rho: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionSnapshot {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub underlying_symbol: String,
    #[serde(default)]
    pub strike_price: f64,
    #[serde(default)]
    pub expiration_date: String,
    #[serde(default)]
    pub contract_type: String,
    pub last_trade: Option<OptionLastTrade>,
    pub last_quote: Option<OptionLastQuote>,
    pub greeks: Option<OptionGreeks>,
    #[serde(rename = "impliedVolatility")]
    pub implied_volatility: Option<f64>,
    #[serde(rename = "dailyBar")]
    pub daily_bar: Option<OptionBar>,
    #[serde(rename = "minuteBar")]
    pub minute_bar: Option<OptionBar>,
    #[serde(rename = "prevDailyBar")]
    pub prev_daily_bar: Option<OptionBar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionSnapshotsResponse {
    pub snapshots: std::collections::HashMap<String, OptionSnapshot>,
    pub next_page_token: Option<String>,
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
        let url = format!("{}/v2/account", self.config.paper_url);
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
        let mut url = format!("{}/v2/assets", self.config.paper_url);
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
    pub async fn get_options_chain(
        &self,
        symbol: &str,
        expiration_date: Option<&str>,
        expiration_date_gte: Option<&str>,
        expiration_date_lte: Option<&str>,
        strike_price_gte: Option<f64>,
        strike_price_lte: Option<f64>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<OptionContractsResponse> {
        info!("Getting option contracts for {}", symbol);
        let mut url = format!(
            "{}/v2/options/contracts?underlying_symbols={}",
            self.config.paper_url, symbol
        );

        if let Some(date) = expiration_date {
            url.push_str(&format!("&expiration_date={}", date));
        }

        if let Some(date) = expiration_date_gte {
            url.push_str(&format!("&expiration_date_gte={}", date));
        }

        if let Some(date) = expiration_date_lte {
            url.push_str(&format!("&expiration_date_lte={}", date));
        }

        if let Some(strike) = strike_price_gte {
            url.push_str(&format!("&strike_price_gte={}", strike));
        }

        if let Some(strike) = strike_price_lte {
            url.push_str(&format!("&strike_price_lte={}", strike));
        }

        if let Some(limit_val) = limit {
            url.push_str(&format!("&limit={}", limit_val));
        }

        if let Some(offset_val) = offset {
            url.push_str(&format!("&offset={}", offset_val));
        }

        // Add a timeout to prevent hanging indefinitely
        let resp = self
            .auth(self.client.get(&url))
            .timeout(std::time::Duration::from_secs(30)) // 30 second timeout
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get options chain: {}", e)))?;

        let data = resp.json::<OptionContractsResponse>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options chain: {}", e))
        })?;

        // Add detailed logging for debugging
        info!("Response parsed successfully. Got {} contracts", data.results.len());
        for (i, contract) in data.results.iter().enumerate().take(3) {
            info!("Sample contract {}: Symbol={}, Strike={}, Exp={}", 
                  i, contract.symbol, contract.strike_price, contract.expiration_date);
        }

        Ok(data)
    }

    /// Get historical options bars data
    pub async fn get_options_bars(
        &self,
        symbols: &[&str],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
        limit: Option<u32>,
        page_token: Option<&str>,
        sort: Option<&str>,
    ) -> Result<OptionBarsResponse> {
        debug!(
            "Getting options bars for symbols: {:?} from {} to {}",
            symbols, start, end
        );
        let symbols_str = symbols.join(",");
        let mut url = format!(
            "{}/v1beta1/options/bars?symbols={}&start={}&end={}&timeframe={}",
            self.config.data_url,
            symbols_str,
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

        let data = resp.json::<OptionBarsResponse>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options bars: {}", e))
        })?;

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
    ) -> Result<OptionTradesResponse> {
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

        let data = resp.json::<OptionTradesResponse>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options trades: {}", e))
        })?;

        Ok(data)
    }

    /// Get latest options quotes
    pub async fn get_options_quotes(&self, symbols: &[&str]) -> Result<OptionQuotesResponse> {
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

        let data = resp.json::<OptionQuotesResponse>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse options quotes: {}", e))
        })?;

        Ok(data)
    }

    /// Get snapshots for a list of option symbols
    pub async fn get_option_snapshots(
        &self,
        symbols: &[&str],
        feed: Option<&str>,
        updated_since: Option<DateTime<Utc>>,
        limit: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<OptionSnapshotsResponse> {
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

        let data = resp.json::<OptionSnapshotsResponse>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse option snapshots: {}", e))
        })?;

        Ok(data)
    }

    /// Get option chain snapshots for an underlying symbol
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
    ) -> Result<OptionSnapshotsResponse> {
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

        // Check if the response is successful
        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_else(|_| "Could not read error response".to_string());
            return Err(OptionsError::Other(format!(
                "Option chain snapshots request failed with status {}: {}",
                status, error_text
            )));
        }

        // Get the response text for debugging
        let resp_text = resp.text().await.map_err(|e| {
            OptionsError::Other(format!("Failed to get response text: {}", e))
        })?;

        // Log the first 200 characters of the response for debugging
        debug!("Option chain snapshots response (first 200 chars): {}", 
               if resp_text.len() > 200 { &resp_text[..200] } else { &resp_text });

        // Parse the response
        let data = serde_json::from_str::<OptionSnapshotsResponse>(&resp_text).map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse option chain snapshots: {} - Response: {}", e, resp_text))
        })?;

        Ok(data)
    }

    /// Get condition codes for options
    pub async fn get_options_condition_codes(&self, tick_type: &str) -> Result<serde_json::Value> {
        debug!(
            "Getting options condition codes for tick type: {}",
            tick_type
        );
        let url = format!(
            "{}/v1beta1/options/meta/conditions/{}",
            self.config.data_url, tick_type
        );

        let resp =
            self.auth(self.client.get(&url)).send().await.map_err(|e| {
                OptionsError::Other(format!("Failed to get condition codes: {}", e))
            })?;

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse condition codes: {}", e))
        })?;

        Ok(data)
    }

    /// Get exchange codes for options
    pub async fn get_options_exchange_codes(&self) -> Result<serde_json::Value> {
        debug!("Getting options exchange codes");
        let url = format!("{}/v1beta1/options/meta/exchanges", self.config.data_url);

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get exchange codes: {}", e)))?;

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse exchange codes: {}", e))
        })?;

        Ok(data)
    }

    /// Get latest option trades
    pub async fn get_latest_options_trades(&self, symbols: &[&str]) -> Result<serde_json::Value> {
        debug!("Getting latest option trades for symbols: {:?}", symbols);
        let symbols_str = symbols.join(",");
        let url = format!(
            "{}/v1beta1/options/trades/latest?symbols={}",
            self.config.data_url, symbols_str
        );

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get latest trades: {}", e)))?;

        let data = resp.json::<serde_json::Value>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse latest trades: {}", e))
        })?;

        Ok(data)
    }

    /// Get latest stock snapshot for a symbol
    pub async fn get_stock_snapshot(&self, symbol: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/v2/stocks/snapshots?symbols={}",
            self.config.data_url, symbol
        );

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get stock snapshot: {}", e)))?;

        let data = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse stock snapshot: {}", e)))?;

        Ok(data)
    }

    /// Get the latest stock quotes for the given symbols
    pub async fn get_latest_stock_quotes(
        &self,
        symbols: &[&str],
    ) -> Result<LatestStockQuotesResponse> {
        let symbols_str = symbols.join(",");
        let url = format!(
            "{}/v2/stocks/quotes/latest?symbols={}",
            self.config.data_url, symbols_str
        );

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get latest stock quotes: {}", e)))?;

        let data = resp.json::<LatestStockQuotesResponse>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse latest stock quotes: {}", e))
        })?;

        Ok(data)
    }

    /// Get the latest stock quote for a single symbol
    pub async fn get_latest_single_stock_quote(
        &self,
        symbol: &str,
        feed: Option<&str>,
        currency: Option<&str>,
    ) -> Result<SingleStockQuoteResponse> {
        let mut url = format!(
            "{}/v2/stocks/{}/quotes/latest",
            self.config.data_url, symbol
        );

        let mut query_params = Vec::new();
        if let Some(feed_val) = feed {
            query_params.push(format!("feed={}", feed_val));
        }
        if let Some(currency_val) = currency {
            query_params.push(format!("currency={}", currency_val));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get latest stock quote: {}", e)))?;

        let data = resp.json::<SingleStockQuoteResponse>().await.map_err(|e| {
            OptionsError::ParseError(format!("Failed to parse latest stock quote: {}", e))
        })?;

        Ok(data)
    }
}
