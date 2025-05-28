use crate::error::{OptionsError, Result};
use crate::models::option::{OptionContract, OptionQuote};
use crate::utils::{delta, implied_volatility, vega};
use ndarray::{Array1, Array2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpliedVolatility {
    pub contract: OptionContract,
    pub value: f64,
    pub underlying_price: f64,
    pub option_price: f64,
    pub time_to_expiration: f64,
    pub delta: f64,
    pub vega: f64,
}

impl ImpliedVolatility {
    pub fn from_quote(
        quote: &OptionQuote,
        risk_free_rate: f64,
        dividend_yield: f64,
    ) -> Result<Self> {
        let contract = &quote.contract;
        let option_price = quote.mid_price();
        let underlying_price = quote.underlying_price;
        let strike = contract.strike;
        let time_to_expiration = contract.time_to_expiration();

        if time_to_expiration <= 0.0 {
            return Err(OptionsError::VolatilityError(
                "Option is expired, cannot calculate implied volatility".to_string(),
            ));
        }

        if option_price <= 0.0 {
            return Err(OptionsError::VolatilityError(
                "Option price must be positive to calculate implied volatility".to_string(),
            ));
        }

        let is_call = contract.is_call();

        let iv = implied_volatility(
            option_price,
            underlying_price,
            strike,
            time_to_expiration,
            risk_free_rate - dividend_yield,
            is_call,
        )
        .map_err(|e| {
            OptionsError::VolatilityError(format!("Failed to calculate implied volatility: {}", e))
        })?;

        let delta_value = delta(
            underlying_price,
            strike,
            time_to_expiration,
            risk_free_rate - dividend_yield,
            iv,
            is_call,
        );

        let vega_value = vega(
            underlying_price,
            strike,
            time_to_expiration,
            risk_free_rate - dividend_yield,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilitySurface {
    pub symbol: String,
    pub expirations: Vec<chrono::DateTime<chrono::Utc>>,
    pub strikes: Vec<f64>,
    pub volatilities: Array2<f64>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub version: u64, // Version counter for tracking changes
}

impl VolatilitySurface {
    pub fn new(symbol: String, implied_volatilities: &[ImpliedVolatility]) -> Result<Self> {
        if implied_volatilities.is_empty() {
            return Err(OptionsError::VolatilityError(
                "Cannot create volatility surface from empty data".to_string(),
            ));
        }

        let mut expirations_set = BTreeSet::new();
        let mut strikes_set: Vec<f64> = Vec::new();

        for iv in implied_volatilities {
            expirations_set.insert(iv.contract.expiration);
            if !strikes_set.contains(&iv.contract.strike) {
                strikes_set.push(iv.contract.strike);
            }
        }

        let expirations: Vec<_> = expirations_set.into_iter().collect();
        let mut strikes = strikes_set;
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less));

        let n_expirations = expirations.len();
        let n_strikes = strikes.len();
        let mut volatilities = Array2::from_elem((n_expirations, n_strikes), f64::NAN);

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
            version: 1, // Initial version
        })
    }

    pub fn interpolate(
        &self,
        expiration: chrono::DateTime<chrono::Utc>,
        strike: f64,
    ) -> Result<f64> {
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

            if v11.is_nan() || v12.is_nan() || v21.is_nan() || v22.is_nan() {
                return Err(OptionsError::VolatilityError(
                    "Cannot interpolate with NaN values".to_string(),
                ));
            }

            let t = (expiration - exp1).num_seconds() as f64 / (exp2 - exp1).num_seconds() as f64;
            let u = (strike - strike1) / (strike2 - strike1);
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

    /// get slice of vol surface for one expiry date
    pub fn slice_by_expiration(
        &self,
        expiration: chrono::DateTime<chrono::Utc>,
    ) -> Result<(Array1<f64>, Array1<f64>)> {
        let exp_idx = self
            .expirations
            .iter()
            .position(|&e| e == expiration)
            .ok_or_else(|| {
                OptionsError::VolatilityError(
                    "Expiration not found in volatility surface".to_string(),
                )
            })?;

        let strikes = Array1::from_vec(self.strikes.clone());
        let volatilities = self.volatilities.slice(ndarray::s![exp_idx, ..]).to_owned();

        Ok((strikes, volatilities))
    }

    /// get slice of vol surface for strike
    pub fn slice_by_strike(&self, strike: f64) -> Result<(Array1<f64>, Array1<f64>)> {
        let strike_idx = self
            .strikes
            .iter()
            .position(|&s| s == strike)
            .ok_or_else(|| {
                OptionsError::VolatilityError("Strike not found in volatility surface".to_string())
            })?;

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
        let volatilities = self
            .volatilities
            .slice(ndarray::s![.., strike_idx])
            .to_owned();

        Ok((times, volatilities))
    }

    /// Update the volatility surface with new implied volatility data
    pub fn update(&mut self, new_ivs: &[ImpliedVolatility]) -> Result<bool> {
        if new_ivs.is_empty() {
            return Ok(false);
        }

        let mut updated = false;
        let mut new_expirations = Vec::new();
        let mut new_strikes = Vec::new();

        for iv in new_ivs {
            let exp = iv.contract.expiration;
            let strike = iv.contract.strike;

            if !self.expirations.contains(&exp) {
                new_expirations.push(exp);
            }

            if !self.strikes.contains(&strike) {
                new_strikes.push(strike);
            }
        }

        if !new_expirations.is_empty() || !new_strikes.is_empty() {
            for exp in &new_expirations {
                self.expirations.push(*exp);
            }
            self.expirations.sort();

            for strike in &new_strikes {
                self.strikes.push(*strike);
            }
            self.strikes
                .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less));

            let n_expirations = self.expirations.len();
            let n_strikes = self.strikes.len();
            let mut new_volatilities = Array2::from_elem((n_expirations, n_strikes), f64::NAN);

            for (i, exp) in self.expirations.iter().enumerate() {
                for (j, strike) in self.strikes.iter().enumerate() {
                    let old_exp_idx = self.expirations.iter().position(|e| e == exp);
                    let old_strike_idx = self.strikes.iter().position(|s| s == strike);

                    if let (Some(old_i), Some(old_j)) = (old_exp_idx, old_strike_idx) {
                        if old_i < self.volatilities.shape()[0]
                            && old_j < self.volatilities.shape()[1]
                        {
                            new_volatilities[[i, j]] = self.volatilities[[old_i, old_j]];
                        }
                    }
                }
            }

            self.volatilities = new_volatilities;
            updated = true;
        }

        for iv in new_ivs {
            let exp_idx = self
                .expirations
                .iter()
                .position(|&e| e == iv.contract.expiration);
            let strike_idx = self.strikes.iter().position(|&s| s == iv.contract.strike);

            if let (Some(i), Some(j)) = (exp_idx, strike_idx) {
                if self.volatilities[[i, j]].is_nan()
                    || (self.volatilities[[i, j]] - iv.value).abs() > 1e-6
                {
                    self.volatilities[[i, j]] = iv.value;
                    updated = true;
                }
            }
        }

        if updated {
            self.timestamp = chrono::Utc::now();
            self.version += 1;
        }

        Ok(updated)
    }

    /// Get the current version of the volatility surface
    pub fn get_version(&self) -> u64 {
        self.version
    }
}
