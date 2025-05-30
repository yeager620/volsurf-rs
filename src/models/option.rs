use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{trace, warn};

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
    pub option_type: OptionType,
    pub strike: f64,
    pub expiration: DateTime<Utc>,
    pub option_symbol: String,
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

    pub fn from_occ_symbol(occ_symbol: &str) -> Option<Self> {
        trace!("Parsing OCC symbol: {}", occ_symbol);
        let mut type_positions = Vec::new();
        for (i, c) in occ_symbol.char_indices() {
            if c == 'C' || c == 'P' {
                type_positions.push(i);
            }
        }

        if type_positions.is_empty() {
            warn!("Failed to find 'C' or 'P' in OCC symbol: {}", occ_symbol);
            return None;
        }

        let mut valid_type_pos = None;
        for &pos in &type_positions {
            if pos >= 6 {
                let date_part = &occ_symbol[(pos - 6)..pos];
                if date_part.chars().all(|c| c.is_digit(10)) {
                    valid_type_pos = Some(pos);
                    break;
                }
            }
        }

        let type_pos = match valid_type_pos {
            Some(pos) => pos,
            None => {
                let pos = *type_positions.last().unwrap();
                warn!("No valid option type position found with date check, using last position {} as fallback for: {}", pos, occ_symbol);
                pos
            }
        };

        if type_pos < 6 {
            warn!(
                "OCC symbol too short for date part: {} (type_pos={})",
                occ_symbol, type_pos
            );
            return None;
        }

        if type_pos + 1 >= occ_symbol.len() {
            warn!("OCC symbol too short for strike price: {}", occ_symbol);
            return None;
        }

        let symbol = occ_symbol[0..(type_pos - 6)].to_string();
        let date_str = &occ_symbol[(type_pos - 6)..type_pos];

        trace!("Extracted symbol: {}, date_str: {}", symbol, date_str);
        let option_type = match occ_symbol.chars().nth(type_pos) {
            Some('C') => OptionType::Call,
            Some('P') => OptionType::Put,
            _ => {
                warn!(
                    "Invalid option type character in OCC symbol: {}",
                    occ_symbol
                );
                return None;
            }
        };
        let strike_str = &occ_symbol[(type_pos + 1)..];
        if strike_str.is_empty() {
            warn!("Empty strike string in OCC symbol: {}", occ_symbol);
            return None;
        }
        let strike = match strike_str.parse::<u32>() {
            Ok(s) => s as f64 / 1000.0,
            Err(e) => {
                warn!(
                    "Failed to parse strike price '{}' in OCC symbol {}: {}",
                    strike_str, occ_symbol, e
                );
                return None;
            }
        };
        if date_str.len() != 6 {
            warn!(
                "Date string '{}' is not exactly 6 characters long in OCC symbol: {}",
                date_str, occ_symbol
            );
            return None;
        }

        let year_str = &date_str[0..2];
        let month_str = &date_str[2..4];
        let day_str = &date_str[4..6];

        trace!(
            "Parsing date components: year={}, month={}, day={}",
            year_str,
            month_str,
            day_str
        );

        let year = match year_str.parse::<i32>() {
            Ok(y) => 2000 + y,
            Err(e) => {
                warn!(
                    "Failed to parse year '{}' in OCC symbol {}: {}",
                    year_str, occ_symbol, e
                );
                return None;
            }
        };

        let month = match month_str.parse::<u32>() {
            Ok(m) if m >= 1 && m <= 12 => m,
            Ok(m) => {
                warn!(
                    "Invalid month value {} (must be 1-12) in OCC symbol: {}",
                    m, occ_symbol
                );
                return None;
            }
            Err(e) => {
                warn!(
                    "Failed to parse month '{}' in OCC symbol {}: {}",
                    month_str, occ_symbol, e
                );
                return None;
            }
        };

        let day = match day_str.parse::<u32>() {
            Ok(d) if d >= 1 && d <= 31 => d,
            Ok(d) => {
                warn!(
                    "Invalid day value {} (must be 1-31) in OCC symbol: {}",
                    d, occ_symbol
                );
                return None;
            }
            Err(e) => {
                warn!(
                    "Failed to parse day '{}' in OCC symbol {}: {}",
                    day_str, occ_symbol, e
                );
                return None;
            }
        };

        let naive_date = match chrono::NaiveDate::from_ymd_opt(year, month, day) {
            Some(d) => d,
            None => {
                warn!(
                    "Invalid date {}-{}-{} in OCC symbol: {}",
                    year, month, day, occ_symbol
                );
                return None;
            }
        };

        let naive_datetime = match naive_date.and_hms_opt(16, 0, 0) {
            Some(dt) => dt,
            None => {
                warn!(
                    "Failed to create datetime for {}-{}-{} 16:00:00 in OCC symbol: {}",
                    year, month, day, occ_symbol
                );
                return None;
            }
        };

        let expiration = match naive_datetime.and_local_timezone(chrono::Utc).single() {
            Some(e) => e,
            None => {
                warn!(
                    "Failed to convert to UTC for {}-{}-{} 16:00:00 in OCC symbol: {}",
                    year, month, day, occ_symbol
                );
                return None;
            }
        };

        trace!(
            "Successfully parsed OCC symbol: {} -> symbol={}, type={:?}, strike={}, expiration={}",
            occ_symbol,
            symbol,
            option_type,
            strike,
            expiration
        );

        Some(Self {
            symbol,
            option_type,
            strike,
            expiration,
            option_symbol: occ_symbol.to_string(),
        })
    }

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
