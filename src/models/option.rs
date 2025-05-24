use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionType {
    Call,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionContract {
    pub symbol: String,
    pub option_type: OptionType, // call vs put
    pub strike: f64,
    pub expiration: DateTime<Utc>,
    pub option_symbol: String, // OCC format
}

impl OptionContract {
    pub fn new(
        symbol: String,
        option_type: OptionType,
        strike: f64,
        expiration: DateTime<Utc>,
    ) -> Self {
        let option_symbol = Self::generate_occ_symbol(&symbol, option_type, strike, expiration);

        Self {
            symbol,
            option_type,
            strike,
            expiration,
            option_symbol,
        }
    }

    /// format: Symbol + YY + MM + DD + C/P + Strike
    /// e.g. AAPL210115C00125000
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
        let strike_str = format!("{:08}", (strike * 1000.0) as u32);
        let date_str = expiration.format("%y%m%d").to_string();
        format!("{}{}{}{}", symbol, date_str, type_char, strike_str)
    }

    /// Parse OCC option symbol
    pub fn from_occ_symbol(occ_symbol: &str) -> Option<Self> {
        let type_pos = occ_symbol.find(|c| c == 'C' || c == 'P')?;
        let symbol = occ_symbol[0..(type_pos - 6)].to_string();
        let date_str = &occ_symbol[(type_pos - 6)..type_pos];
        let option_type = match occ_symbol.chars().nth(type_pos)? {
            'C' => OptionType::Call,
            'P' => OptionType::Put,
            _ => return None,
        };
        let strike_str = &occ_symbol[(type_pos + 1)..];
        let strike = strike_str.parse::<u32>().ok()? as f64 / 1000.0;

        let year = 2000 + date_str[0..2].parse::<i32>().ok()?;
        let month = date_str[2..4].parse::<u32>().ok()?;
        let day = date_str[4..6].parse::<u32>().ok()?;

        let expiration = chrono::NaiveDate::from_ymd_opt(year, month, day)?
            .and_hms_opt(16, 0, 0)? // Options expire at 4:00 PM ET
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

    pub fn is_call(&self) -> bool {
        self.option_type == OptionType::Call
    }
    pub fn is_put(&self) -> bool {
        self.option_type == OptionType::Put
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionQuote {
    pub contract: OptionContract,
    pub bid: f64,
    pub ask: f64,
    pub last: f64,
    pub volume: u64,
    pub open_interest: u64,
    pub underlying_price: f64,
    pub timestamp: DateTime<Utc>,
}

impl OptionQuote {
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
    pub fn mid_price(&self) -> f64 {
        (self.bid + self.ask) / 2.0
    }
}
