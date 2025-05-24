use statrs::distribution::{Normal, Univariate};

/// Calculate the Black-Scholes option price
fn price(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> f64 {
    let n = Normal::new(0.0, 1.0).unwrap();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    if is_call {
        s * n.cdf(d1) - k * (-r * t).exp() * n.cdf(d2)
    } else {
        k * (-r * t).exp() * n.cdf(-d2) - s * n.cdf(-d1)
    }
}

/// Delta of an option
pub fn delta(s: f64, k: f64, t: f64, r: f64, sigma: f64, is_call: bool) -> f64 {
    let n = Normal::new(0.0, 1.0).unwrap();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    if is_call {
        n.cdf(d1)
    } else {
        n.cdf(d1) - 1.0
    }
}

/// Vega of an option
pub fn vega(s: f64, k: f64, t: f64, r: f64, sigma: f64) -> f64 {
    let n = Normal::new(0.0, 1.0).unwrap();
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    s * n.pdf(d1) * t.sqrt()
}

/// Compute implied volatility using a simple Newton-Raphson method
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
    let mut sigma = 0.3_f64;
    for _ in 0..100 {
        let price = price(s, k, t, r, sigma, is_call);
        let diff = price - price_target;
        if diff.abs() < 1e-8 {
            return Ok(sigma);
        }
        let v = vega(s, k, t, r, sigma);
        if v.abs() < 1e-8 {
            break;
        }
        sigma -= diff / v;
        if sigma <= 0.0 {
            sigma = 1e-4;
        }
    }
    Err("Implied volatility did not converge".to_string())
}
