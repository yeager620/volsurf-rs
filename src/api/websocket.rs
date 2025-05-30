use crate::config::AlpacaConfig;
use crate::error::{OptionsError, Result};
use crate::models::{OptionContract, OptionQuote as ModelOptionQuote, OptionType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
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
    pub s: String,
    pub bp: f64,
    pub bs: u64,
    pub ap: f64,
    #[serde(rename = "as")]
    pub as_size: u64,
    pub t: DateTime<Utc>,
    #[serde(default)]
    pub up: f64,
    pub option_symbol: String,
    pub strike: f64,
    pub expiration: DateTime<Utc>,
    pub option_type: OptionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionTrade {
    pub s: String,
    pub p: f64,
    pub sz: u64,
    pub t: DateTime<Utc>,
    pub x: String,
    #[serde(default)]
    pub up: f64,
    pub option_symbol: String,
    pub strike: f64,
    pub expiration: DateTime<Utc>,
    pub option_type: OptionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionBar {
    pub s: String,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: u64,
    pub t: DateTime<Utc>,
    pub vw: f64,
    #[serde(default)]
    pub up: f64,
    pub option_symbol: String,
    pub strike: f64,
    pub expiration: DateTime<Utc>,
    pub option_type: OptionType,
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

    #[allow(dead_code)]
    fn option_trades(mut self, symbols: Vec<String>) -> Self {
        self.trades = Some(symbols);
        self
    }

    #[allow(dead_code)]
    fn option_bars(mut self, symbols: Vec<String>) -> Self {
        self.bars = Some(symbols);
        self
    }
}

pub struct WebSocketClient {
    config: AlpacaConfig,
    data_sender: mpsc::Sender<ModelOptionQuote>,
    data_receiver: Arc<Mutex<mpsc::Receiver<ModelOptionQuote>>>,
    notification_tx: Arc<tokio::sync::broadcast::Sender<()>>,
}

impl WebSocketClient {
    pub fn new(config: AlpacaConfig) -> Self {
        let (data_sender, data_receiver) = mpsc::channel(1000);
        let (notification_tx, _) = tokio::sync::broadcast::channel(100);

        Self {
            config,
            data_sender,
            data_receiver: Arc::new(Mutex::new(data_receiver)),
            notification_tx: Arc::new(notification_tx),
        }
    }

    pub fn get_notification_channel(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.notification_tx.subscribe()
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

        let data_url = &self.config.data_url;
        let ws_domain = if data_url.starts_with("https://") {
            data_url
                .strip_prefix("https://")
                .unwrap_or("data.alpaca.markets")
        } else {
            "data.alpaca.markets"
        };

        let ws_url = format!("wss://{}/v1beta1/options", ws_domain);
        info!("Using WebSocket URL: {}", ws_url);

        let url = Url::parse(&ws_url).map_err(|e| {
            OptionsError::WebSocketError(format!("Failed to parse WebSocket URL: {}", e))
        })?;

        let sender = self.data_sender.clone();
        let api_key = self.config.api_key.clone();
        let api_secret = self.config.api_secret.clone();
        let symbols_clone = symbols.clone();
        let notification_tx = self.notification_tx.clone();

        fn get_status_from_error(
            err: &tokio_tungstenite::tungstenite::Error,
        ) -> Option<reqwest::StatusCode> {
            use tokio_tungstenite::tungstenite::Error;
            match err {
                Error::Http(response) => {
                    Some(reqwest::StatusCode::from_u16(response.status().as_u16()).ok()?)
                }
                _ => None,
            }
        }

        tokio::spawn(async move {
            info!(
                "Starting options data stream for {} symbols",
                symbols_clone.len()
            );

            let url_str = url.to_string();
            let (ws_stream, response) = match connect_async(url_str).await {
                Ok(conn) => conn,
                Err(e) => {
                    let error_msg = format!("Failed to connect to WebSocket: {}", e);
                    warn!("{}", error_msg);

                    if let Some(status) = get_status_from_error(&e) {
                        warn!(
                            "HTTP error: {} {}",
                            status.as_u16(),
                            status.canonical_reason().unwrap_or("Unknown")
                        );

                        if status == reqwest::StatusCode::NOT_FOUND {
                            warn!("The WebSocket endpoint was not found (404). This could be because:");
                            warn!("1. The WebSocket URL is incorrect");
                            warn!("2. The Alpaca API has changed");
                            warn!("3. Your Alpaca subscription doesn't include options data");
                        } else if status == reqwest::StatusCode::UNAUTHORIZED {
                            warn!("Authentication failed (401). Please check your API key and secret.");
                        } else if status == reqwest::StatusCode::FORBIDDEN {
                            warn!("Access forbidden (403). Your account may not have access to options data.");
                        }
                    }

                    return;
                }
            };

            info!("WebSocket connected with status: {}", response.status());
            debug!("WebSocket response headers: {:?}", response.headers());

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

            if let Err(e) = write.send(Message::Text(auth_json.into())).await {
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

            if let Err(e) = write.send(Message::Text(subscribe_json.into())).await {
                warn!("Failed to send subscribe message: {}", e);
                return;
            }

            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        debug!("Received text message");

                        if text.contains(r#""T":"q""#) {
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
                                        Ok(_) => {
                                            if let Err(e) = notification_tx.send(()) {
                                                debug!("Failed to send notification: {}", e);
                                            }
                                        }
                                        Err(mpsc::error::TrySendError::Full(model_quote)) => {
                                            if sender.send(model_quote).await.is_err() {
                                                warn!("Failed to send quote to channel");
                                                break;
                                            }
                                            if let Err(e) = notification_tx.send(()) {
                                                debug!("Failed to send notification: {}", e);
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

    pub async fn next_option_quotes_batch(
        &self,
        max_batch_size: usize,
    ) -> Result<Vec<ModelOptionQuote>> {
        let mut receiver = self.data_receiver.lock().await;
        let mut quotes = Vec::with_capacity(max_batch_size);

        if let Some(quote) = receiver.recv().await {
            quotes.push(quote);
        } else {
            return Ok(quotes);
        }

        while quotes.len() < max_batch_size {
            match receiver.try_recv() {
                Ok(quote) => quotes.push(quote),
                Err(_) => break,
            }
        }

        Ok(quotes)
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
