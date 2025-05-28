use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SurfaceUpdate {
    pub strikes: Vec<f64>,
    pub expiries: Vec<NaiveDate>,
    pub sigma: Vec<f64>,
}
