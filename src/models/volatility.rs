//! Volatility models and calculations
//!
//! This module contains data structures and functions for implied volatility
//! and volatility surface calculations.

use crate::error::{OptionsError, Result};
use crate::models::option::{OptionContract, OptionQuote, OptionType};
use black_scholes::{delta, implied_volatility, vega};
use ndarray::{Array1, Array2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Implied volatility calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpliedVolatility {
    /// Option contract
    pub contract: OptionContract,
    /// Implied volatility value
    pub value: f64,
    /// Underlying price
    pub underlying_price: f64,
    /// Option price used for calculation
    pub option_price: f64,
    /// Time to expiration in years
    pub time_to_expiration: f64,
    /// Delta of the option
    pub delta: f64,
    /// Vega of the option
    pub vega: f64,
}

impl ImpliedVolatility {
    /// Calculate implied volatility from an option quote
    pub fn from_quote(quote: &OptionQuote, risk_free_rate: f64) -> Result<Self> {
        let contract = &quote.contract;
        let option_price = quote.mid_price();
        let underlying_price = quote.underlying_price;
        let strike = contract.strike;
        let time_to_expiration = contract.time_to_expiration();
        
        // If option is expired, we can't calculate IV
        if time_to_expiration <= 0.0 {
            return Err(OptionsError::VolatilityError(
                "Option is expired, cannot calculate implied volatility".to_string(),
            ));
        }
        
        // If option price is zero or negative, we can't calculate IV
        if option_price <= 0.0 {
            return Err(OptionsError::VolatilityError(
                "Option price must be positive to calculate implied volatility".to_string(),
            ));
        }
        
        let is_call = contract.is_call();
        
        // Calculate implied volatility
        let iv = implied_volatility(
            option_price,
            underlying_price,
            strike,
            time_to_expiration,
            risk_free_rate,
            is_call,
        )
        .map_err(|e| {
            OptionsError::VolatilityError(format!("Failed to calculate implied volatility: {}", e))
        })?;
        
        // Calculate delta
        let delta_value = delta(
            underlying_price,
            strike,
            time_to_expiration,
            risk_free_rate,
            iv,
            is_call,
        );
        
        // Calculate vega
        let vega_value = vega(
            underlying_price,
            strike,
            time_to_expiration,
            risk_free_rate,
            iv,
        );
        
        Ok(Self {
            contract: contract.clone(),
            value: iv,
            underlying_price,
            option_price,
            time_to_expiration,
            delta: delta_value,
            vega: vega_value,
        })
    }
}

/// Volatility surface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilitySurface {
    /// Underlying symbol
    pub symbol: String,
    /// Expiration dates
    pub expirations: Vec<chrono::DateTime<chrono::Utc>>,
    /// Strike prices
    pub strikes: Vec<f64>,
    /// Implied volatility values (2D array: expirations x strikes)
    pub volatilities: Array2<f64>,
    /// Timestamp when the surface was created
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl VolatilitySurface {
    /// Create a new volatility surface from a collection of implied volatilities
    pub fn new(
        symbol: String,
        implied_volatilities: &[ImpliedVolatility],
    ) -> Result<Self> {
        if implied_volatilities.is_empty() {
            return Err(OptionsError::VolatilityError(
                "Cannot create volatility surface from empty data".to_string(),
            ));
        }
        
        // Extract unique expirations and strikes
        let mut expiration_map = HashMap::new();
        let mut strike_map = HashMap::new();
        
        for iv in implied_volatilities {
            let expiration = iv.contract.expiration;
            let strike = iv.contract.strike;
            
            expiration_map.insert(expiration, ());
            strike_map.insert(strike, ());
        }
        
        // Sort expirations and strikes
        let mut expirations: Vec<_> = expiration_map.keys().cloned().collect();
        let mut strikes: Vec<_> = strike_map.keys().cloned().collect();
        
        expirations.sort();
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        // Create a 2D array for volatilities
        let n_expirations = expirations.len();
        let n_strikes = strikes.len();
        let mut volatilities = Array2::from_elem((n_expirations, n_strikes), f64::NAN);
        
        // Fill the volatility array
        for iv in implied_volatilities {
            let expiration = iv.contract.expiration;
            let strike = iv.contract.strike;
            
            let exp_idx = expirations.iter().position(|&e| e == expiration);
            let strike_idx = strikes.iter().position(|&s| s == strike);
            
            if let (Some(i), Some(j)) = (exp_idx, strike_idx) {
                volatilities[[i, j]] = iv.value;
            }
        }
        
        Ok(Self {
            symbol,
            expirations,
            strikes,
            volatilities,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Interpolate implied volatility for a specific expiration and strike
    pub fn interpolate(&self, expiration: chrono::DateTime<chrono::Utc>, strike: f64) -> Result<f64> {
        // Find the nearest expirations
        let mut exp_idx_before = None;
        let mut exp_idx_after = None;
        
        for (i, &exp) in self.expirations.iter().enumerate() {
            if exp <= expiration {
                exp_idx_before = Some(i);
            } else {
                exp_idx_after = Some(i);
                break;
            }
        }
        
        // Find the nearest strikes
        let mut strike_idx_before = None;
        let mut strike_idx_after = None;
        
        for (i, &s) in self.strikes.iter().enumerate() {
            if s <= strike {
                strike_idx_before = Some(i);
            } else {
                strike_idx_after = Some(i);
                break;
            }
        }
        
        // Perform bilinear interpolation if we have all four corners
        if let (Some(e1), Some(e2), Some(s1), Some(s2)) = (
            exp_idx_before,
            exp_idx_after,
            strike_idx_before,
            strike_idx_after,
        ) {
            let exp1 = self.expirations[e1];
            let exp2 = self.expirations[e2];
            let strike1 = self.strikes[s1];
            let strike2 = self.strikes[s2];
            
            let v11 = self.volatilities[[e1, s1]];
            let v12 = self.volatilities[[e1, s2]];
            let v21 = self.volatilities[[e2, s1]];
            let v22 = self.volatilities[[e2, s2]];
            
            // Check if any of the corner values are NaN
            if v11.is_nan() || v12.is_nan() || v21.is_nan() || v22.is_nan() {
                return Err(OptionsError::VolatilityError(
                    "Cannot interpolate with NaN values".to_string(),
                ));
            }
            
            // Calculate weights
            let t = (expiration - exp1).num_seconds() as f64 / (exp2 - exp1).num_seconds() as f64;
            let u = (strike - strike1) / (strike2 - strike1);
            
            // Bilinear interpolation
            let v = (1.0 - t) * (1.0 - u) * v11
                + (1.0 - t) * u * v12
                + t * (1.0 - u) * v21
                + t * u * v22;
            
            Ok(v)
        } else {
            Err(OptionsError::VolatilityError(
                "Cannot interpolate: expiration or strike out of range".to_string(),
            ))
        }
    }

    /// Get a slice of the volatility surface for a specific expiration
    pub fn slice_by_expiration(&self, expiration: chrono::DateTime<chrono::Utc>) -> Result<(Array1<f64>, Array1<f64>)> {
        let exp_idx = self.expirations.iter().position(|&e| e == expiration).ok_or_else(|| {
            OptionsError::VolatilityError("Expiration not found in volatility surface".to_string())
        })?;
        
        let strikes = Array1::from_vec(self.strikes.clone());
        let volatilities = self.volatilities.slice(ndarray::s![exp_idx, ..]).to_owned();
        
        Ok((strikes, volatilities))
    }

    /// Get a slice of the volatility surface for a specific strike
    pub fn slice_by_strike(&self, strike: f64) -> Result<(Array1<f64>, Array1<f64>)> {
        let strike_idx = self.strikes.iter().position(|&s| s == strike).ok_or_else(|| {
            OptionsError::VolatilityError("Strike not found in volatility surface".to_string())
        })?;
        
        // Convert expirations to time to expiration in years
        let now = chrono::Utc::now();
        let times_to_expiration: Vec<f64> = self
            .expirations
            .iter()
            .map(|&exp| {
                if exp <= now {
                    0.0
                } else {
                    (exp - now).num_seconds() as f64 / (365.0 * 24.0 * 60.0 * 60.0)
                }
            })
            .collect();
        
        let times = Array1::from_vec(times_to_expiration);
        let volatilities = self.volatilities.slice(ndarray::s![.., strike_idx]).to_owned();
        
        Ok((times, volatilities))
    }
}