use thiserror::Error;

/// Custom error types for the options-rs library
#[derive(Error, Debug)]
pub enum OptionsError {

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Volatility calculation error: {0}")]
    VolatilityError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("WebSocket connection error: {0}")]
    WebSocketError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, OptionsError>;
