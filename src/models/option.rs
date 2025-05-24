//! Option contract models
//!
//! This module contains data structures for representing options contracts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Option type (Call or Put)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionType {
    /// Call option
    Call,
    /// Put option
    Put,
}

impl std::fmt::Display for OptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OptionType::Call => write!(f, "Call"),
            OptionType::Put => write!(f, "Put"),
        }
    }
}

/// Option contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionContract {
    /// Underlying symbol
    pub symbol: String,
    /// Option type (Call or Put)
    pub option_type: OptionType,
    /// Strike price
    pub strike: f64,
    /// Expiration date
    pub expiration: DateTime<Utc>,
    /// Option symbol (OCC format)
    pub option_symbol: String,
}

impl OptionContract {
    /// Create a new option contract
    pub fn new(
        symbol: String,
        option_type: OptionType,
        strike: f64,
        expiration: DateTime<Utc>,
    ) -> Self {
        // Generate OCC option symbol
        let option_symbol = Self::generate_occ_symbol(&symbol, option_type, strike, expiration);
        
        Self {
            symbol,
            option_type,
            strike,
            expiration,
            option_symbol,
        }
    }

    /// Generate OCC option symbol
    ///
    /// Format: Symbol + YY + MM + DD + C/P + Strike
    /// Example: AAPL210115C00125000
    fn generate_occ_symbol(
        symbol: &str,
        option_type: OptionType,
        strike: f64,
        expiration: DateTime<Utc>,
    ) -> String {
        let type_char = match option_type {
            OptionType::Call => 'C',
            OptionType::Put => 'P',
        };
        
        // Format strike price as 8 digits with leading zeros
        let strike_str = format!("{:08}", (strike * 1000.0) as u32);
        
        // Format date as YYMMDD
        let date_str = expiration.format("%y%m%d").to_string();
        
        format!("{}{}{}{}", symbol, date_str, type_char, strike_str)
    }

    /// Parse OCC option symbol
    pub fn from_occ_symbol(occ_symbol: &str) -> Option<Self> {
        // OCC symbols have format: Symbol + YY + MM + DD + C/P + Strike
        // Example: AAPL210115C00125000
        
        // Find the position of C or P
        let type_pos = occ_symbol.find(|c| c == 'C' || c == 'P')?;
        
        // Symbol is everything before the date (which is 6 chars before type)
        let symbol = occ_symbol[0..(type_pos - 6)].to_string();
        
        // Date is 6 chars before the type
        let date_str = &occ_symbol[(type_pos - 6)..type_pos];
        
        // Type is C or P
        let option_type = match occ_symbol.chars().nth(type_pos)? {
            'C' => OptionType::Call,
            'P' => OptionType::Put,
            _ => return None,
        };
        
        // Strike is everything after the type, divided by 1000
        let strike_str = &occ_symbol[(type_pos + 1)..];
        let strike = strike_str.parse::<u32>().ok()? as f64 / 1000.0;
        
        // Parse date
        let year = 2000 + date_str[0..2].parse::<i32>().ok()?;
        let month = date_str[2..4].parse::<u32>().ok()?;
        let day = date_str[4..6].parse::<u32>().ok()?;
        
        // Create DateTime
        let expiration = chrono::NaiveDate::from_ymd_opt(year, month, day)?
            .and_hms_opt(16, 0, 0)?  // Options expire at 4:00 PM ET
            .and_local_timezone(chrono::Utc)
            .single()?;
        
        Some(Self {
            symbol,
            option_type,
            strike,
            expiration,
            option_symbol: occ_symbol.to_string(),
        })
    }

    /// Calculate time to expiration in years
    pub fn time_to_expiration(&self) -> f64 {
        let now = Utc::now();
        if now > self.expiration {
            return 0.0;
        }
        
        let duration = self.expiration - now;
        duration.num_seconds() as f64 / (365.0 * 24.0 * 60.0 * 60.0)
    }

    /// Check if the option is a call
    pub fn is_call(&self) -> bool {
        self.option_type == OptionType::Call
    }

    /// Check if the option is a put
    pub fn is_put(&self) -> bool {
        self.option_type == OptionType::Put
    }
}

/// Option quote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionQuote {
    /// Option contract
    pub contract: OptionContract,
    /// Bid price
    pub bid: f64,
    /// Ask price
    pub ask: f64,
    /// Last price
    pub last: f64,
    /// Volume
    pub volume: u64,
    /// Open interest
    pub open_interest: u64,
    /// Underlying price
    pub underlying_price: f64,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl OptionQuote {
    /// Create a new option quote
    pub fn new(
        contract: OptionContract,
        bid: f64,
        ask: f64,
        last: f64,
        volume: u64,
        open_interest: u64,
        underlying_price: f64,
    ) -> Self {
        Self {
            contract,
            bid,
            ask,
            last,
            volume,
            open_interest,
            underlying_price,
            timestamp: Utc::now(),
        }
    }

    /// Get the mid price
    pub fn mid_price(&self) -> f64 {
        (self.bid + self.ask) / 2.0
    }
}