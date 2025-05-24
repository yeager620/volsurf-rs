use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use crate::models::{OptionContract, OptionQuote as ModelOptionQuote, OptionType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};
use chrono::{DateTime, Utc};
use futures::StreamExt;

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
    #[serde(rename = "as")]
    pub as_size: u64,
    /// Timestamp
    pub t: DateTime<Utc>,
    /// Underlying price
    #[serde(default)]
    pub up: f64,
    /// Option symbol
    pub option_symbol: String,
    /// Strike price
    pub strike: f64,
    /// Expiration date
    pub expiration: DateTime<Utc>,
    /// Option type
    pub option_type: OptionType,
}

/// Option trade data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionTrade {
    /// Symbol
    pub s: String,
    /// Price
    pub p: f64,
    /// Size
    pub sz: u64,
    /// Timestamp
    pub t: DateTime<Utc>,
    /// Exchange
    pub x: String,
    /// Underlying price
    #[serde(default)]
    pub up: f64,
    /// Option symbol
    pub option_symbol: String,
    /// Strike price
    pub strike: f64,
    /// Expiration date
    pub expiration: DateTime<Utc>,
    /// Option type
    pub option_type: OptionType,
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
    /// Volume weighted average price
    pub vw: f64,
    /// Underlying price
    #[serde(default)]
    pub up: f64,
    /// Option symbol
    pub option_symbol: String,
    /// Strike price
    pub strike: f64,
    /// Expiration date
    pub expiration: DateTime<Utc>,
    /// Option type
    pub option_type: OptionType,
}

/// Authentication message
#[derive(Debug, Serialize)]
struct Auth {
    action: String,
    key: String,
    secret: String,
}

impl Auth {
    /// Create a new authentication message
    fn new(key: String, secret: String) -> Self {
        Self {
            action: "auth".to_string(),
            key,
            secret,
        }
    }
}

/// Subscription message
#[derive(Debug, Serialize)]
struct Subscribe {
    action: String,
    quotes: Option<Vec<String>>,
    trades: Option<Vec<String>>,
    bars: Option<Vec<String>>,
}

impl Subscribe {
    /// Create a new subscription
    fn new() -> Self {
        Self {
            action: "subscribe".to_string(),
            quotes: None,
            trades: None,
            bars: None,
        }
    }

    /// Add option quotes to the subscription
    fn option_quotes(mut self, symbols: Vec<String>) -> Self {
        self.quotes = Some(symbols);
        self
    }

    /// Add option trades to the subscription
    fn option_trades(mut self, symbols: Vec<String>) -> Self {
        self.trades = Some(symbols);
        self
    }

    /// Add option bars to the subscription
    fn option_bars(mut self, symbols: Vec<String>) -> Self {
        self.bars = Some(symbols);
        self
    }
}

/// WebSocket client for Alpaca Markets API
pub struct WebSocketClient {
    /// Configuration for the Alpaca API
    config: AlpacaConfig,
    /// Channel for sending market data
    data_sender: mpsc::Sender<ModelOptionQuote>,
    /// Channel for receiving market data
    data_receiver: Arc<Mutex<mpsc::Receiver<ModelOptionQuote>>>,
}

impl WebSocketClient {
    /// Create a new WebSocket client
    pub fn new(config: AlpacaConfig) -> Self {
        // Increase channel buffer size for better handling of high-frequency updates
        // This helps prevent backpressure when receiving many messages quickly
        let (data_sender, data_receiver) = mpsc::channel(1000);

        Self {
            config,
            data_sender,
            data_receiver: Arc::new(Mutex::new(data_receiver)),
        }
    }

    /// Connect to the WebSocket and start streaming data
    pub async fn connect(&self, symbols: Vec<String>) -> Result<()> {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
        use url::Url;

        info!("Connecting to Alpaca WebSocket for options data");
        debug!("Options symbols to subscribe: {:?}", symbols);

        if symbols.is_empty() {
            return Err(OptionsError::WebSocketError(
                "No symbols provided for subscription".to_string(),
            ));
        }

        let ws_url = format!("{}/v2/options", self.config.data_url);
        debug!("WebSocket URL: {}", ws_url);

        let url = Url::parse(&ws_url).map_err(|e| {
            OptionsError::WebSocketError(format!("Failed to parse WebSocket URL: {}", e))
        })?;

        let sender = self.data_sender.clone();
        let api_key = self.config.api_key.clone();
        let api_secret = self.config.api_secret.clone();
        let symbols_clone = symbols.clone();

        tokio::spawn(async move {
            info!("Starting options data stream for {} symbols", symbols_clone.len());

            // Connect to WebSocket
            let (ws_stream, _) = match connect_async(url).await {
                Ok(conn) => conn,
                Err(e) => {
                    warn!("Failed to connect to WebSocket: {}", e);
                    return;
                }
            };

            info!("WebSocket connected");

            let (mut write, mut read) = ws_stream.split();

            // Authenticate
            let auth_msg = Auth::new(api_key, api_secret);
            let auth_json = match serde_json::to_string(&auth_msg) {
                Ok(json) => json,
                Err(e) => {
                    warn!("Failed to serialize auth message: {}", e);
                    return;
                }
            };

            if let Err(e) = write.send(Message::Text(auth_json)).await {
                warn!("Failed to send auth message: {}", e);
                return;
            }

            // Subscribe to option quotes
            let subscribe_msg = Subscribe::new().option_quotes(symbols_clone);
            let subscribe_json = match serde_json::to_string(&subscribe_msg) {
                Ok(json) => json,
                Err(e) => {
                    warn!("Failed to serialize subscribe message: {}", e);
                    return;
                }
            };

            if let Err(e) = write.send(Message::Text(subscribe_json)).await {
                warn!("Failed to send subscribe message: {}", e);
                return;
            }

            // Process incoming messages
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        // Only log message content at trace level to reduce overhead
                        debug!("Received text message");

                        // Try to parse directly as OptionQuote first for efficiency
                        // This avoids the overhead of parsing to Value and then to OptionQuote
                        if text.contains(r#""T":"q""#) {
                            // Fast path for option quotes
                            if let Ok(quote) = serde_json::from_str::<OptionQuote>(&text) {
                                // Create option contract from the quote data
                                if let Some(contract) = OptionContract::from_occ_symbol(&quote.option_symbol) {
                                    // Create a model quote with the contract
                                    let model_quote = ModelOptionQuote::new(
                                        contract,
                                        quote.bp,
                                        quote.ap,
                                        (quote.bp + quote.ap) / 2.0, // mid price
                                        0, // Volume not provided in quote
                                        0, // Open interest not provided
                                        quote.up,
                                    );

                                    // Use try_send instead of send to avoid awaiting when possible
                                    // This reduces latency by not yielding to the executor
                                    match sender.try_send(model_quote) {
                                        Ok(_) => {},
                                        Err(mpsc::error::TrySendError::Full(model_quote)) => {
                                            // Channel is full, fall back to awaiting
                                            if sender.send(model_quote).await.is_err() {
                                                warn!("Failed to send quote to channel");
                                                break;
                                            }
                                        },
                                        Err(_) => {
                                            warn!("Failed to send quote to channel");
                                            break;
                                        }
                                    }
                                }
                                continue;
                            }
                        }

                        // Fallback path for other message types
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(json) => {
                                // Check if it's a market data message
                                if let Some(msg_type) = json.get("T") {
                                    match msg_type.as_str() {
                                        Some("q") => {
                                            // Option quote - should be handled by fast path above
                                            debug!("Quote message fell back to slow path");
                                        },
                                        Some("t") => {
                                            // Option trade - could be processed similarly
                                            debug!("Received option trade");
                                        },
                                        Some("b") => {
                                            // Option bar - could be processed similarly
                                            debug!("Received option bar");
                                        },
                                        Some("subscription") => {
                                            info!("Subscription confirmed");
                                        },
                                        Some("error") => {
                                            warn!("Received error: {}", json);
                                        },
                                        Some(t) => {
                                            debug!("Received unknown message type: {}", t);
                                        },
                                        None => {
                                            debug!("Received message without type");
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                warn!("Failed to parse message: {}", e);
                            }
                        }
                    },
                    Ok(Message::Binary(_)) => {
                        debug!("Received binary message");
                    },
                    Ok(Message::Ping(data)) => {
                        // Respond to ping with pong
                        if let Err(e) = write.send(Message::Pong(data)).await {
                            warn!("Failed to send pong: {}", e);
                            break;
                        }
                    },
                    Ok(Message::Pong(_)) => {
                        debug!("Received pong");
                    },
                    Ok(Message::Close(_)) => {
                        info!("WebSocket closed");
                        break;
                    },
                    Ok(Message::Frame(_)) => {
                        debug!("Received frame message");
                    },
                    Err(e) => {
                        warn!("WebSocket error: {}", e);
                        break;
                    }
                }
            }

            info!("WebSocket connection closed");
        });

        Ok(())
    }

    /// Get the next option quote
    pub async fn next_option_quote(&self) -> Result<Option<ModelOptionQuote>> {
        let mut receiver = self.data_receiver.lock().await;

        match receiver.recv().await {
            Some(quote) => Ok(Some(quote)),
            None => Ok(None),
        }
    }

    /// Process option quotes with a callback function
    pub async fn process_option_quotes<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(ModelOptionQuote) -> Result<()>,
    {
        let mut receiver = self.data_receiver.lock().await;

        while let Some(quote) = receiver.recv().await {
            callback(quote)?;
        }

        Ok(())
    }
}

// Implement conversion from API types to model types
impl From<OptionQuote> for ModelOptionQuote {
    fn from(quote: OptionQuote) -> Self {
        let mid_price = (quote.bp + quote.ap) / 2.0;

        // Create contract from option symbol
        if let Some(contract) = OptionContract::from_occ_symbol(&quote.option_symbol) {
            Self::new(
                contract,
                quote.bp,
                quote.ap,
                mid_price,
                0, // Volume not provided in quote
                0, // Open interest not provided
                quote.up,
            )
        } else {
            // Fallback to creating a contract from the available fields
            let contract = OptionContract::new(
                quote.s.clone(),
                quote.option_type,
                quote.strike,
                quote.expiration,
            );

            Self::new(
                contract,
                quote.bp,
                quote.ap,
                mid_price,
                0, // Volume not provided in quote
                0, // Open interest not provided
                quote.up,
            )
        }
    }
}
