pub struct RiskNeutralDensity {
    pub strikes: Vec<f64>,
    pub density: Vec<f64>,
}

use crate::models::OptionQuote;

/// Compute a simple risk-neutral probability density function from call prices.
pub fn risk_neutral_density(quotes: &[OptionQuote], risk_free_rate: f64) -> Option<RiskNeutralDensity> {
    if quotes.len() < 3 {
        return None;
    }

    let mut data: Vec<(f64, f64)> = quotes
        .iter()
        .filter(|q| q.contract.is_call())
        .map(|q| (q.contract.strike, q.mid_price()))
        .collect();

    if data.len() < 3 {
        return None;
    }

    data.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let strikes: Vec<f64> = data.iter().map(|(k, _)| *k).collect();
    let prices: Vec<f64> = data.iter().map(|(_, p)| *p).collect();

    // Assume all quotes have same expiration
    let t = quotes[0].contract.time_to_expiration();
    if t <= 0.0 {
        return None;
    }

    let discount = (risk_free_rate * t).exp();

    let n = strikes.len();
    let mut density = vec![0.0; n];

    for i in 1..n - 1 {
        let k_prev = strikes[i - 1];
        let k_next = strikes[i + 1];
        let c_prev = prices[i - 1];
        let c = prices[i];
        let c_next = prices[i + 1];

        let h = (k_next - k_prev) / 2.0;
        if h > 0.0 {
            let second = (c_next - 2.0 * c + c_prev) / (h * h);
            let val = discount * second;
            if val.is_finite() && val > 0.0 {
                density[i] = val;
            }
        }
    }

    // Normalize using trapezoidal rule
    let mut total = 0.0;
    for i in 1..n {
        let dx = strikes[i] - strikes[i - 1];
        total += 0.5 * (density[i] + density[i - 1]) * dx;
    }

    if total > 0.0 {
        for d in &mut density {
            *d /= total;
        }
    }

    Some(RiskNeutralDensity { strikes, density })
}
