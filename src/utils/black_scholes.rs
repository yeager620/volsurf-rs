use statrs::distribution::ContinuousCDF;
use statrs::distribution::{Continuous, Normal};
use std::sync::OnceLock;

static NORMAL_DIST: OnceLock<Normal> = OnceLock::new();

fn get_normal() -> &'static Normal {
    NORMAL_DIST.get_or_init(|| Normal::new(0.0, 1.0).unwrap())
}

/// Black-Scholes option price
fn price(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> f64 {
    let n = get_normal();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    if is_call {
        s * n.cdf(d1) - k * (-r * t).exp() * n.cdf(d2)
    } else {
        k * (-r * t).exp() * n.cdf(-d2) - s * n.cdf(-d1)
    }
}

pub fn delta(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> f64 {
    let n = get_normal();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    if is_call {
        n.cdf(d1)
    } else {
        n.cdf(d1) - 1.0
    }
}

pub fn vega(s: f64, k: f64, t: f64, r: f64, sigma: f64) -> f64 {
    let n = get_normal();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    s * n.pdf(d1) * t.sqrt()
}

fn initial_sigma_guess(price_target: f64, s: f64, k: f64, t: f64, is_call: bool) -> f64 {
    let moneyness = if is_call { s / k } else { k / s }; // Brenner-Subrahmanyam approximation
                                                         // deep ITM / OTM adjustment
    if moneyness > 1.5 || moneyness < 0.5 {
        return 0.3;
    }
    (2.0 * std::f64::consts::PI / t).sqrt() * (price_target / s) // approximation for ATM
}

/// Newton-Raphson method
pub fn implied_volatility(
    price_target: f64,
    s: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
) -> Result<f64, String> {
    if price_target <= 0.0 || t <= 0.0 || s <= 0.0 || k <= 0.0 {
        return Err("Invalid input".to_string());
    }
    let mut sigma = initial_sigma_guess(price_target, s, k, t, is_call);
    for i in 0..50 {
        let price = price(s, k, t, r, sigma, is_call);
        let diff = price - price_target;
        let tolerance = if i < 10 { 1e-8 } else { 1e-6 };
        if diff.abs() < tolerance {
            return Ok(sigma);
        }

        let v = vega(s, k, t, r, sigma);
        if v.abs() < 1e-8 {
            break;
        }
        let step = diff / v;
        sigma -= if i < 5 { step * 0.5 } else { step };

        if sigma <= 0.0 {
            sigma = 1e-4;
        } else if sigma > 5.0 {
            sigma = 5.0;
        }
    }
    let final_price = price(s, k, t, r, sigma, is_call);
    let final_diff = (final_price - price_target).abs();
    if final_diff < 0.01 * price_target {
        return Ok(sigma);
    }

    Err("Implied volatility did not converge".to_string())
}

/// Batch calculation of IVs using rayon
pub fn batch_implied_volatility(
    quotes: &[(f64, f64, f64, f64, bool)], // (price, s, k, t, is_call)
    r: f64,
) -> Vec<Result<f64, String>> {
    use rayon::prelude::*;

    quotes
        .par_iter()
        .map(|(price, s, k, t, is_call)| implied_volatility(*price, *s, *k, *t, r, *is_call))
        .collect()
}
