use plotly::{Plot, Surface};
use crate::models::volatility::VolatilitySurface;

pub fn surface_to_plot(surface: &VolatilitySurface) -> Plot {
    let z: Vec<Vec<f64>> = surface
        .volatilities
        .outer_iter()
        .map(|row| row.to_vec())
        .collect();

    let trace = Surface::new(z)
        .x(surface.strikes.clone())
        .y(surface.expirations.iter().map(|e| e.timestamp() as f64).collect());

    let mut plot = Plot::new();
    plot.add_trace(trace);
    plot
}
