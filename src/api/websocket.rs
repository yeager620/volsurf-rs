//! WebSocket client for Alpaca Markets API
//!
//! This module provides a client for streaming real-time data from Alpaca Markets.

use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use crate::models::option::OptionContract;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error, info, warn};
use chrono::{DateTime, Utc};

/// Market data types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "T")]
pub enum MarketData {
    /// Option quote
    #[serde(rename = "q")]
    OptionQuote(OptionQuote),
    /// Option trade
    #[serde(rename = "t")]
    OptionTrade(OptionTrade),
    /// Option bar
    #[serde(rename = "b")]
    OptionBar(OptionBar),
}

/// Option quote data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionQuote {
    /// Symbol
    pub s: String,
    /// Bid price
    pub bp: f64,
    /// Bid size
    pub bs: u64,
    /// Ask price
    pub ap: f64,
    /// Ask size
    pub as_: u64,
    /// Timestamp
    pub t: DateTime<Utc>,
}

/// Option trade data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionTrade {
    /// Symbol
    pub s: String,
    /// Price
    pub p: f64,
    /// Size
    pub z: u64,
    /// Timestamp
    pub t: DateTime<Utc>,
}

/// Option bar data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionBar {
    /// Symbol
    pub s: String,
    /// Open price
    pub o: f64,
    /// High price
    pub h: f64,
    /// Low price
    pub l: f64,
    /// Close price
    pub c: f64,
    /// Volume
    pub v: u64,
    /// Timestamp
    pub t: DateTime<Utc>,
}

/// Subscription message
#[derive(Debug, Serialize)]
struct Subscribe {
    action: String,
    options: Vec<String>,
}

impl Subscribe {
    /// Create a new subscription
    fn new() -> Self {
        Self {
            action: "subscribe".to_string(),
            options: Vec::new(),
        }
    }

    /// Add option quotes to the subscription
    fn option_quotes(mut self, symbols: Vec<String>) -> Self {
        self.options.extend(symbols);
        self
    }

    /// Add option trades to the subscription
    fn option_trades(self, _symbols: Vec<String>) -> Self {
        // In this simplified implementation, we subscribe to all data types
        // for the symbols specified in option_quotes
        self
    }

    /// Add option bars to the subscription
    fn option_bars(self, _symbols: Vec<String>) -> Self {
        // In this simplified implementation, we subscribe to all data types
        // for the symbols specified in option_quotes
        self
    }
}

/// WebSocket client for Alpaca Markets API
pub struct WebSocketClient {
    /// Configuration for the Alpaca API
    config: AlpacaConfig,
    /// Channel for sending market data
    data_sender: mpsc::Sender<MarketData>,
    /// Channel for receiving market data
    data_receiver: Arc<Mutex<mpsc::Receiver<MarketData>>>,
}

impl WebSocketClient {
    /// Create a new WebSocket client
    pub fn new(config: AlpacaConfig) -> Self {
        let (data_sender, data_receiver) = mpsc::channel(100);

        Self {
            config,
            data_sender,
            data_receiver: Arc::new(Mutex::new(data_receiver)),
        }
    }

    /// Connect to the WebSocket and start streaming data
    pub async fn connect(&self, symbols: Vec<String>) -> Result<()> {
        info!("Connecting to Alpaca WebSocket");

        let stream = MarketDataStream::with_credentials(
            &self.config.api_key,
            &self.config.api_secret,
        )
        .await
        .map_err(|e| OptionsError::WebSocketError(format!("Failed to connect: {}", e)))?;

        // Split the stream into sink and stream parts
        let (mut sink, mut stream) = stream.split();

        // Subscribe to option quotes, trades, and bars for the specified symbols
        let subscription = Subscribe::new()
            .option_quotes(symbols.clone())
            .option_trades(symbols.clone())
            .option_bars(symbols);

        debug!("Subscribing to: {:?}", subscription);

        sink.send(subscription.into())
            .await
            .map_err(|e| OptionsError::WebSocketError(format!("Failed to subscribe: {}", e)))?;

        // Clone the sender for the task
        let sender = self.data_sender.clone();

        // Spawn a task to process incoming messages
        tokio::spawn(async move {
            info!("WebSocket stream started");

            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(data) => {
                        debug!("Received market data: {:?}", data);

                        if let Err(e) = sender.send(data).await {
                            error!("Failed to send market data: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("WebSocket error: {}", e);
                    }
                }
            }

            info!("WebSocket stream ended");
        });

        Ok(())
    }

    /// Get the next option quote
    pub async fn next_option_quote(&self) -> Result<Option<OptionQuote>> {
        let mut receiver = self.data_receiver.lock().await;

        while let Some(data) = receiver.recv().await {
            if let MarketData::OptionQuote(quote) = data {
                return Ok(Some(quote));
            }
        }

        Ok(None)
    }

    /// Get the next option trade
    pub async fn next_option_trade(&self) -> Result<Option<OptionTrade>> {
        let mut receiver = self.data_receiver.lock().await;

        while let Some(data) = receiver.recv().await {
            if let MarketData::OptionTrade(trade) = data {
                return Ok(Some(trade));
            }
        }

        Ok(None)
    }

    /// Get the next option bar
    pub async fn next_option_bar(&self) -> Result<Option<OptionBar>> {
        let mut receiver = self.data_receiver.lock().await;

        while let Some(data) = receiver.recv().await {
            if let MarketData::OptionBar(bar) = data {
                return Ok(Some(bar));
            }
        }

        Ok(None)
    }

    /// Process option quotes with a callback function
    pub async fn process_option_quotes<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(OptionQuote) -> Result<()>,
    {
        let mut receiver = self.data_receiver.lock().await;

        while let Some(data) = receiver.recv().await {
            if let MarketData::OptionQuote(quote) = data {
                callback(quote)?;
            }
        }

        Ok(())
    }
}
