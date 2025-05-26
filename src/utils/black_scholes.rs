use statrs::distribution::ContinuousCDF;
use statrs::distribution::{Continuous, Normal};
use std::sync::OnceLock;

static NORMAL_DIST: OnceLock<Normal> = OnceLock::new();

fn get_normal() -> &'static Normal {
    NORMAL_DIST.get_or_init(|| Normal::new(0.0, 1.0).unwrap())
}

/// Calculate d1 parameter for Black-Scholes model
fn calculate_d1(s: f64, k: f64, t: f64, r: f64, sigma: f64) -> f64 {
    ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt())
}

/// Calculate d2 parameter for Black-Scholes model
fn calculate_d2(d1: f64, sigma: f64, t: f64) -> f64 {
    d1 - sigma * t.sqrt()
}

/// Black-Scholes option price
pub fn price(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> f64 {
    let n = get_normal();
    let d1 = calculate_d1(s, k, t, r, sigma);
    let d2 = calculate_d2(d1, sigma, t);
    if is_call {
        s * n.cdf(d1) - k * (-r * t).exp() * n.cdf(d2)
    } else {
        k * (-r * t).exp() * n.cdf(-d2) - s * n.cdf(-d1)
    }
}

pub fn delta(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> f64 {
    let n = get_normal();
    let d1 = calculate_d1(s, k, t, r, sigma);
    if is_call {
        n.cdf(d1)
    } else {
        n.cdf(d1) - 1.0
    }
}

pub fn vega(s: f64, k: f64, t: f64, r: f64, sigma: f64) -> f64 {
    let n = get_normal();
    let d1 = calculate_d1(s, k, t, r, sigma);
    s * n.pdf(d1) * t.sqrt()
}

/// Calculate intrinsic value of an option
fn calculate_intrinsic(s: f64, k: f64, is_call: bool) -> f64 {
    if is_call {
        (s - k).max(0.0)
    } else {
        (k - s).max(0.0)
    }
}



/// Newton-Raphson method with improved convergence and special handling for call options
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

    // Calculate intrinsic value
    let intrinsic = calculate_intrinsic(s, k, is_call);

    // If price is below intrinsic (due to data issues), adjust it
    let adjusted_price = price_target.max(intrinsic);

    // Initial guess
    let mut sigma = 0.2;
    let mut sigma_low = 1e-4;
    let mut sigma_high = 5.0;

    for _ in 0..100 {
        let price = price(s, k, t, r, sigma, is_call);
        let diff = price - adjusted_price;

        if diff.abs() < 1e-6 {
            return Ok(sigma);
        }

        if diff > 0.0 {
            sigma_high = sigma;
        } else {
            sigma_low = sigma;
        }

        let v = vega(s, k, t, r, sigma);

        if v.abs() > 1e-8 {
            let new_sigma = sigma - diff / v;
            if new_sigma > sigma_low && new_sigma < sigma_high {
                sigma = new_sigma;
            } else {
                sigma = (sigma_low + sigma_high) / 2.0;
            }
        } else {
            sigma = (sigma_low + sigma_high) / 2.0;
        }
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
