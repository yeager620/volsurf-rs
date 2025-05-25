use serde::{Serialize, Deserialize};
use chrono::NaiveDate;

/// Sent on every surface refresh.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SurfaceUpdate {
    pub strikes: Vec<f64>,
    pub expiries: Vec<NaiveDate>,
    /// Row-major σ matrix: z[row * strikes.len() + col]
    pub sigma: Vec<f64>,
}
