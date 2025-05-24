use thiserror::Error;

/// Custom error types for the options-rs library
#[derive(Error, Debug)]
pub enum OptionsError {

    /// Error when parsing data
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Error when calculating implied volatility
    #[error("Volatility calculation error: {0}")]
    VolatilityError(String),

    /// Error when loading configuration
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Error when connecting to WebSocket
    #[error("WebSocket connection error: {0}")]
    WebSocketError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization/Deserialization error
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

/// Result type alias for options-rs operations
pub type Result<T> = std::result::Result<T, OptionsError>;
