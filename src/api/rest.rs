//! REST client for Alpaca Markets API
//!
//! This module provides a client for interacting with the Alpaca Markets REST API.

use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use apca::api::v2::asset::Asset;
use apca::api::v2::order::{Order, OrderReq};
use apca::Client;
use chrono::{DateTime, Utc};
use tracing::{debug, info};

/// REST client for Alpaca Markets API
pub struct RestClient {
    /// Alpaca API client
    client: Client,
    /// Configuration for the Alpaca API
    config: AlpacaConfig,
}

impl RestClient {
    /// Create a new REST client
    pub fn new(config: AlpacaConfig) -> Self {
        let client = Client::new(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.base_url.clone(),
            config.data_url.clone(),
        );

        Self { client, config }
    }

    /// Get account information
    pub async fn get_account(&self) -> Result<apca::api::v2::account::Account> {
        debug!("Getting account information");
        self.client
            .issue::<apca::api::v2::account::Account>(&apca::api::v2::account::Get::new())
            .await
            .map_err(OptionsError::AlpacaError)
    }

    /// Get assets
    pub async fn get_assets(&self, asset_class: Option<&str>) -> Result<Vec<Asset>> {
        debug!("Getting assets");
        let mut req = apca::api::v2::asset::List::new();
        
        if let Some(class) = asset_class {
            req = req.asset_class(class);
        }
        
        self.client
            .issue::<Vec<Asset>>(&req)
            .await
            .map_err(OptionsError::AlpacaError)
    }

    /// Get options for a symbol
    pub async fn get_options(&self, symbol: &str) -> Result<Vec<Asset>> {
        info!("Getting options for symbol: {}", symbol);
        self.get_assets(Some("option"))
            .await
            .map(|assets| {
                assets
                    .into_iter()
                    .filter(|asset| asset.symbol.contains(symbol))
                    .collect()
            })
    }

    /// Get historical options data
    pub async fn get_options_bars(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timeframe: &str,
    ) -> Result<serde_json::Value> {
        debug!(
            "Getting options bars for {} from {} to {}",
            symbol, start, end
        );
        
        // Construct the URL for the options bars endpoint
        let url = format!(
            "{}/v2/stocks/{}/bars?start={}&end={}&timeframe={}",
            self.config.data_url,
            symbol,
            start.to_rfc3339(),
            end.to_rfc3339(),
            timeframe
        );

        // Make the request
        let response = reqwest::Client::new()
            .get(&url)
            .header("APCA-API-KEY-ID", &self.config.api_key)
            .header("APCA-API-SECRET-KEY", &self.config.api_secret)
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get options bars: {}", e)))?;

        // Parse the response
        let data = response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| OptionsError::ParseError(format!("Failed to parse options bars: {}", e)))?;

        Ok(data)
    }

    /// Place an order
    pub async fn place_order(&self, order_req: OrderReq) -> Result<Order> {
        debug!("Placing order: {:?}", order_req);
        self.client
            .issue::<Order>(&apca::api::v2::order::Post::new(order_req))
            .await
            .map_err(OptionsError::AlpacaError)
    }
}
