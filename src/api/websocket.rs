use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use crate::models::{OptionContract, OptionQuote as ModelOptionQuote, OptionType};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "T")]
pub enum MarketData {
    #[serde(rename = "q")]
    OptionQuote(OptionQuote),
    #[serde(rename = "t")]
    OptionTrade(OptionTrade),
    #[serde(rename = "b")]
    OptionBar(OptionBar),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionQuote {
    pub s: String, // Symbol
    pub bp: f64,   // Bid price
    pub bs: u64,   // Bid size
    pub ap: f64,   // Ask price
    #[serde(rename = "as")]
    pub as_size: u64, // Ask size
    pub t: DateTime<Utc>, // Timestamp
    #[serde(default)]
    pub up: f64, // Underlying price
    pub option_symbol: String, // Option symbol
    pub strike: f64, // Strike price
    pub expiration: DateTime<Utc>, // Expiration date
    pub option_type: OptionType, // Option type
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionTrade {
    pub s: String,        // Symbol
    pub p: f64,           // Price
    pub sz: u64,          // Size
    pub t: DateTime<Utc>, // Timestamp
    pub x: String,        // Exchange
    #[serde(default)]
    pub up: f64, // Underlying price
    pub option_symbol: String, // Option symbol
    pub strike: f64,      // Strike price
    pub expiration: DateTime<Utc>, // Expiration date
    pub option_type: OptionType, // Option type
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionBar {
    pub s: String,        // Symbol
    pub o: f64,           // Open price
    pub h: f64,           // High price
    pub l: f64,           // Low price
    pub c: f64,           // Close price
    pub v: u64,           // Volume
    pub t: DateTime<Utc>, // Timestamp
    pub vw: f64,          // Volume weighted average price
    #[serde(default)]
    pub up: f64, // Underlying price
    pub option_symbol: String, // Option symbol
    pub strike: f64,      // Strike price
    pub expiration: DateTime<Utc>, // Expiration date
    pub option_type: OptionType, // Option type
}

#[derive(Debug, Serialize)]
struct Auth {
    action: String,
    key: String,
    secret: String,
}

impl Auth {
    fn new(key: String, secret: String) -> Self {
        Self {
            action: "auth".to_string(),
            key,
            secret,
        }
    }
}

#[derive(Debug, Serialize)]
struct Subscribe {
    action: String,
    quotes: Option<Vec<String>>,
    trades: Option<Vec<String>>,
    bars: Option<Vec<String>>,
}

impl Subscribe {
    fn new() -> Self {
        Self {
            action: "subscribe".to_string(),
            quotes: None,
            trades: None,
            bars: None,
        }
    }

    fn option_quotes(mut self, symbols: Vec<String>) -> Self {
        self.quotes = Some(symbols);
        self
    }

    fn option_trades(mut self, symbols: Vec<String>) -> Self {
        self.trades = Some(symbols);
        self
    }

    fn option_bars(mut self, symbols: Vec<String>) -> Self {
        self.bars = Some(symbols);
        self
    }
}

pub struct WebSocketClient {
    config: AlpacaConfig,                        // Configuration for the Alpaca API
    data_sender: mpsc::Sender<ModelOptionQuote>, // Channel for sending market data
    data_receiver: Arc<Mutex<mpsc::Receiver<ModelOptionQuote>>>, // Channel for receiving market data
}

impl WebSocketClient {
    pub fn new(config: AlpacaConfig) -> Self {
        let (data_sender, data_receiver) = mpsc::channel(1000);

        Self {
            config,
            data_sender,
            data_receiver: Arc::new(Mutex::new(data_receiver)),
        }
    }

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

        let ws_url = "wss://stream.data.alpaca.markets/v1beta1/options";
        let url = Url::parse(ws_url).map_err(|e| {
            OptionsError::WebSocketError(format!("Failed to parse WebSocket URL: {}", e))
        })?;

        let sender = self.data_sender.clone();
        let api_key = self.config.api_key.clone();
        let api_secret = self.config.api_secret.clone();
        let symbols_clone = symbols.clone();

        tokio::spawn(async move {
            info!(
                "Starting options data stream for {} symbols",
                symbols_clone.len()
            );

            let (ws_stream, _) = match connect_async(url).await {
                Ok(conn) => conn,
                Err(e) => {
                    warn!("Failed to connect to WebSocket: {}", e);
                    return;
                }
            };

            info!("WebSocket connected");

            let (mut write, mut read) = ws_stream.split();

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

            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        debug!("Received text message");

                        if text.contains(r#""T":"q""#) {
                            // Fast path for quote messages
                            if let Ok(quote) = serde_json::from_str::<OptionQuote>(&text) {
                                if let Some(contract) =
                                    OptionContract::from_occ_symbol(&quote.option_symbol)
                                {
                                    let model_quote = ModelOptionQuote::new(
                                        contract,
                                        quote.bp,
                                        quote.ap,
                                        (quote.bp + quote.ap) / 2.0,
                                        0,
                                        0,
                                        quote.up,
                                    );

                                    match sender.try_send(model_quote) {
                                        Ok(_) => {}
                                        Err(mpsc::error::TrySendError::Full(model_quote)) => {
                                            if sender.send(model_quote).await.is_err() {
                                                warn!("Failed to send quote to channel");
                                                break;
                                            }
                                        }
                                        Err(_) => {
                                            warn!("Failed to send quote to channel");
                                            break;
                                        }
                                    }
                                }
                                continue;
                            }
                        }

                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(json) => {
                                if let Some(msg_type) = json.get("T") {
                                    match msg_type.as_str() {
                                        Some("q") => debug!("Quote message fell back to slow path"),
                                        Some("t") => debug!("Received option trade"),
                                        Some("b") => debug!("Received option bar"),
                                        Some("subscription") => info!("Subscription confirmed"),
                                        Some("error") => warn!("Received error: {}", json),
                                        Some(t) => debug!("Received unknown message type: {}", t),
                                        None => debug!("Received message without type"),
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse message: {}", e);
                            }
                        }
                    }
                    Ok(Message::Binary(_)) => {
                        debug!("Received binary message");
                    }
                    Ok(Message::Ping(data)) => {
                        if let Err(e) = write.send(Message::Pong(data)).await {
                            warn!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Ok(Message::Pong(_)) => {
                        debug!("Received pong");
                    }
                    Ok(Message::Close(_)) => {
                        info!("WebSocket closed");
                        break;
                    }
                    Ok(Message::Frame(_)) => {
                        debug!("Received frame message");
                    }
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

    pub async fn next_option_quote(&self) -> Result<Option<ModelOptionQuote>> {
        let mut receiver = self.data_receiver.lock().await;

        match receiver.recv().await {
            Some(quote) => Ok(Some(quote)),
            None => Ok(None),
        }
    }

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

impl From<OptionQuote> for ModelOptionQuote {
    fn from(quote: OptionQuote) -> Self {
        let mid_price = (quote.bp + quote.ap) / 2.0;

        if let Some(contract) = OptionContract::from_occ_symbol(&quote.option_symbol) {
            Self::new(contract, quote.bp, quote.ap, mid_price, 0, 0, quote.up)
        } else {
            // Fallback to creating a contract from available fields
            let contract = OptionContract::new(
                quote.s.clone(),
                quote.option_type,
                quote.strike,
                quote.expiration,
            );

            Self::new(contract, quote.bp, quote.ap, mid_price, 0, 0, quote.up)
        }
    }
}
