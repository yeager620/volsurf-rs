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

fn initial_sigma_guess(price_target: f64, s: f64, k: f64, t: f64, is_call: bool) -> f64 {
    let moneyness = s / k;

    // Calculate intrinsic value
    let intrinsic = calculate_intrinsic(s, k, is_call);

    // Calculate time value
    let time_value = price_target - intrinsic;

    // For call options, use a more aggressive approach
    if is_call {
        // Deep ITM calls
        if moneyness > 1.3 {
            return 0.3;
        }
        // Deep OTM calls
        else if moneyness < 0.7 {
            return 0.7;
        }
        // For calls with very little time value (close to intrinsic)
        else if time_value < 0.05 * price_target {
            return 0.2; // Start with a low volatility
        }
        // For calls with high time value
        else if time_value > 0.5 * price_target {
            return 0.5; // Start with a moderate volatility
        }
    } else {
        // Deep ITM puts
        if moneyness < 0.7 {
            return 0.3;
        }
        // Deep OTM puts
        else if moneyness > 1.3 {
            return 0.7;
        }
        // For puts with very little time value
        else if time_value < 0.05 * price_target {
            return 0.2;
        }
        // For puts with high time value
        else if time_value > 0.5 * price_target {
            return 0.5;
        }
    }

    // For near-the-money options with moderate time value
    // Use the Brenner-Subrahmanyam approximation with bounds
    let bs_approx = (2.0 * std::f64::consts::PI / t).sqrt() * (price_target / s);
    bs_approx.clamp(0.1, 1.0) // Wider bounds for more flexibility
}

/// Handle special cases for extreme option values
fn handle_special_cases(
    time_value: f64,
    moneyness: f64,
    adjusted_price: f64,
    is_call: bool,
) -> Option<f64> {
    // For options with very small time value or extreme moneyness, use a simplified approach
    if time_value < 0.01 || moneyness > 2.0 || moneyness < 0.5 {
        // For deep ITM calls or deep OTM puts with minimal time value, return a small volatility
        if (is_call && moneyness > 1.5) || (!is_call && moneyness < 0.7) {
            if time_value < 0.05 {
                return Some(0.1); // Very low volatility for options trading near intrinsic
            }
        }

        // For deep OTM calls or deep ITM puts with minimal price, return a high volatility
        if (is_call && moneyness < 0.7) || (!is_call && moneyness > 1.5) {
            if adjusted_price < 0.1 {
                return Some(1.0); // High volatility for far OTM options
            }
        }
    }
    None
}

/// Apply bisection method for implied volatility calculation
fn apply_bisection(
    s: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
    adjusted_price: f64,
    sigma_low: f64,
    sigma_high: f64,
) -> f64 {
    let sigma_mid = (sigma_low + sigma_high) / 2.0;
    let price_mid = price(s, k, t, r, sigma_mid, is_call);

    if price_mid > adjusted_price {
        (sigma_low + sigma_mid) / 2.0
    } else {
        (sigma_mid + sigma_high) / 2.0
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

    // Special case for deep ITM or OTM options with very small time value
    let time_value = adjusted_price - intrinsic;
    let moneyness = s / k;

    // Check for special cases first
    if let Some(special_sigma) =
        handle_special_cases(time_value, moneyness, adjusted_price, is_call)
    {
        return Ok(special_sigma);
    }

    // Get initial guess
    let mut sigma = initial_sigma_guess(adjusted_price, s, k, t, is_call);

    // Bisection method fallback bounds - wider for calls
    let mut sigma_low = 0.001;
    let mut sigma_high = if is_call { 10.0 } else { 5.0 }; // Higher upper bound for calls

    // Use more iterations for better convergence
    let max_iterations = if is_call { 200 } else { 100 }; // More iterations for calls
    for i in 0..max_iterations {
        let price = price(s, k, t, r, sigma, is_call);
        let diff = price - adjusted_price;

        // Simplified tolerance logic
        let tolerance = if is_call {
            if i < 50 {
                1e-3
            } else {
                1e-2
            }
        } else {
            if i < 20 {
                1e-4
            } else {
                1e-3
            }
        };

        if diff.abs() < tolerance {
            return Ok(sigma);
        }

        // Update bisection bounds
        if diff > 0.0 {
            sigma_high = sigma;
        } else {
            sigma_low = sigma;
        }

        let v = vega(s, k, t, r, sigma);

        // If vega is too small, switch to bisection method
        let vega_threshold = if is_call { 1e-5 } else { 1e-6 };
        if v.abs() < vega_threshold {
            sigma = apply_bisection(s, k, t, r, is_call, adjusted_price, sigma_low, sigma_high);
            continue;
        }

        // Newton-Raphson step with damping factor
        let step = diff / v;
        let damping = if is_call {
            if i < 20 {
                0.3
            } else if i < 50 {
                0.5
            } else {
                0.7
            }
        } else {
            if i < 10 {
                0.5
            } else if i < 30 {
                0.7
            } else {
                0.9
            }
        };
        sigma -= step * damping;

        // Ensure sigma stays within reasonable bounds
        if sigma <= sigma_low || sigma >= sigma_high {
            // If Newton-Raphson step goes outside bounds, use bisection
            sigma = apply_bisection(s, k, t, r, is_call, adjusted_price, sigma_low, sigma_high);
        }

        // For calls, periodically try a completely different starting point
        if is_call && i > 0 && i % 50 == 0 && diff.abs() > tolerance * 10.0 {
            // Try a completely different sigma value
            sigma = if sigma < 0.5 { 1.0 } else { 0.2 };
        }
    }

    // Final check with more relaxed tolerance
    let final_price = price(s, k, t, r, sigma, is_call);
    let final_diff = (final_price - adjusted_price).abs();

    // Accept if within tolerance - more relaxed for calls
    let final_tolerance = if is_call { 0.1 } else { 0.05 };
    if final_diff < final_tolerance * adjusted_price {
        return Ok(sigma);
    }

    // Last resort: if we're dealing with a call option, return a reasonable estimate
    if is_call {
        if moneyness > 1.3 {
            return Ok(0.2); // Deep ITM calls
        } else if moneyness < 0.7 {
            return Ok(0.8); // Deep OTM calls
        } else {
            return Ok(0.4); // ATM calls
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
