//! Plotting utilities for options data
//!
//! This module provides functions for visualizing options data,
//! including volatility surfaces and option chains.

use crate::error::{OptionsError, Result};
use crate::models::volatility::VolatilitySurface;
use ndarray::Array1;
use plotters::prelude::*;
use std::path::Path;

/// Plot a volatility smile (volatility vs. strike for a single expiration)
pub fn plot_volatility_smile<P: AsRef<Path>>(
    strikes: &Array1<f64>,
    volatilities: &Array1<f64>,
    symbol: &str,
    expiration: &chrono::DateTime<chrono::Utc>,
    output_path: P,
) -> Result<()> {
    let output_path = output_path.as_ref();
    
    // Filter out NaN values
    let mut valid_points: Vec<(f64, f64)> = Vec::new();
    for (i, &vol) in volatilities.iter().enumerate() {
        if !vol.is_nan() {
            valid_points.push((strikes[i], vol));
        }
    }
    
    if valid_points.is_empty() {
        return Err(OptionsError::Other(
            "No valid data points for volatility smile plot".to_string(),
        ));
    }
    
    // Find min/max values for axes
    let min_strike = valid_points.iter().map(|(s, _)| *s).fold(f64::INFINITY, f64::min);
    let max_strike = valid_points.iter().map(|(s, _)| *s).fold(f64::NEG_INFINITY, f64::max);
    let min_vol = valid_points.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
    let max_vol = valid_points.iter().map(|(_, v)| *v).fold(f64::NEG_INFINITY, f64::max);
    
    // Add some padding
    let strike_range = max_strike - min_strike;
    let vol_range = max_vol - min_vol;
    let strike_min = min_strike - 0.05 * strike_range;
    let strike_max = max_strike + 0.05 * strike_range;
    let vol_min = (min_vol - 0.1 * vol_range).max(0.0);  // IV can't be negative
    let vol_max = max_vol + 0.1 * vol_range;
    
    // Format expiration date
    let exp_str = expiration.format("%Y-%m-%d").to_string();
    
    // Create the plot
    let root = BitMapBackend::new(output_path, (800, 600)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| OptionsError::Other(e.to_string()))?;
    
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("{} Volatility Smile - {}", symbol, exp_str),
            ("sans-serif", 30).into_font(),
        )
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(strike_min..strike_max, vol_min..vol_max)
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    chart
        .configure_mesh()
        .x_desc("Strike Price")
        .y_desc("Implied Volatility")
        .axis_desc_style(("sans-serif", 15))
        .draw()
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Draw the volatility smile
    chart
        .draw_series(LineSeries::new(
            valid_points.iter().map(|&(s, v)| (s, v)),
            &BLUE,
        ))
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Draw points
    chart
        .draw_series(
            valid_points
                .iter()
                .map(|&(s, v)| Circle::new((s, v), 3, BLUE.filled())),
        )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Add a note about the date
    root
        .draw_text(
        &format!("Generated: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")),
        &TextStyle::from(("sans-serif", 15)).color(&BLACK),
        (10, 570),
    )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    root.present().map_err(|e| OptionsError::Other(e.to_string()))?;

    Ok(())
}


/// Plot a volatility term structure (volatility vs. time to expiration for a single strike)
pub fn plot_volatility_term_structure<P: AsRef<Path>>(
    times: &Array1<f64>,
    volatilities: &Array1<f64>,
    symbol: &str,
    strike: f64,
    output_path: P,
) -> Result<()> {
    let output_path = output_path.as_ref();
    
    // Filter out NaN values
    let mut valid_points: Vec<(f64, f64)> = Vec::new();
    for (i, &vol) in volatilities.iter().enumerate() {
        if !vol.is_nan() {
            valid_points.push((times[i], vol));
        }
    }
    
    if valid_points.is_empty() {
        return Err(OptionsError::Other(
            "No valid data points for volatility term structure plot".to_string(),
        ));
    }
    
    // Find min/max values for axes
    let min_time = valid_points.iter().map(|(t, _)| *t).fold(f64::INFINITY, f64::min);
    let max_time = valid_points.iter().map(|(t, _)| *t).fold(f64::NEG_INFINITY, f64::max);
    let min_vol = valid_points.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
    let max_vol = valid_points.iter().map(|(_, v)| *v).fold(f64::NEG_INFINITY, f64::max);
    
    // Add some padding
    let time_range = max_time - min_time;
    let vol_range = max_vol - min_vol;
    let time_min = min_time.max(0.0);  // Time can't be negative
    let time_max = max_time + 0.05 * time_range;
    let vol_min = (min_vol - 0.1 * vol_range).max(0.0);  // IV can't be negative
    let vol_max = max_vol + 0.1 * vol_range;
    
    // Create the plot
    let root = BitMapBackend::new(output_path, (800, 600)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| OptionsError::Other(e.to_string()))?;
    
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("{} Volatility Term Structure - Strike ${:.2}", symbol, strike),
            ("sans-serif", 30).into_font(),
        )
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(time_min..time_max, vol_min..vol_max)
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    chart
        .configure_mesh()
        .x_desc("Time to Expiration (Years)")
        .y_desc("Implied Volatility")
        .axis_desc_style(("sans-serif", 15))
        .draw().map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Draw the term structure
    chart
        .draw_series(LineSeries::new(
            valid_points.iter().map(|&(t, v)| (t, v)),
            &BLUE,
        ))
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Draw points
    chart
        .draw_series(
            valid_points
                .iter()
                .map(|&(t, v)| Circle::new((t, v), 3, BLUE.filled())),
        )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Add a note about the date
    root
        .draw_text(
        &format!("Generated: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")),
        &TextStyle::from(("sans-serif", 15)).color(&BLACK),
        (10, 570),
    )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    root.present().map_err(|e| OptionsError::Other(e.to_string()))?;
    
    Ok(())
}

/// Plot a volatility surface (3D plot of volatility vs. strike and time to expiration)
pub fn plot_volatility_surface<P: AsRef<Path>>(
    surface: &VolatilitySurface,
    output_path: P,
) -> Result<()> {
    let output_path = output_path.as_ref();
    
    // Convert expirations to time to expiration in years
    let now = chrono::Utc::now();
    let times_to_expiration: Vec<f64> = surface
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
    
    // Find min/max values for axes
    let min_strike = surface.strikes.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_strike = surface.strikes.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let min_time = times_to_expiration.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_time = times_to_expiration.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    
    // Find min/max volatility, ignoring NaN values
    let mut min_vol = f64::INFINITY;
    let mut max_vol = f64::NEG_INFINITY;
    for &vol in surface.volatilities.iter() {
        if !vol.is_nan() {
            min_vol = min_vol.min(vol);
            max_vol = max_vol.max(vol);
        }
    }
    
    // Add some padding
    let strike_range = max_strike - min_strike;
    let time_range = max_time - min_time;
    let vol_range = max_vol - min_vol;
    let strike_min = min_strike - 0.05 * strike_range;
    let strike_max = max_strike + 0.05 * strike_range;
    let time_min = min_time.max(0.0);  // Time can't be negative
    let time_max = max_time + 0.05 * time_range;
    let vol_min = (min_vol - 0.1 * vol_range).max(0.0);  // IV can't be negative
    let vol_max = max_vol + 0.1 * vol_range;
    
    // Create the plot
    let root = BitMapBackend::new(output_path, (800, 600)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Since plotters doesn't have built-in 3D surface plots, we'll create a heatmap instead
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("{} Volatility Surface", surface.symbol),
            ("sans-serif", 30).into_font(),
        )
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(strike_min..strike_max, time_min..time_max)
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    chart
        .configure_mesh()
        .x_desc("Strike Price")
        .y_desc("Time to Expiration (Years)")
        .axis_desc_style(("sans-serif", 15))
        .draw().map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Create a color gradient for the heatmap
    let color_gradient = colorous::VIRIDIS;
    
    // Draw the heatmap
    for (i, &time) in times_to_expiration.iter().enumerate() {
        for (j, &strike) in surface.strikes.iter().enumerate() {
            let vol = surface.volatilities[[i, j]];
            if !vol.is_nan() {
                // Normalize volatility to [0, 1] for color mapping
                let normalized_vol = (vol - vol_min) / (vol_max - vol_min);
                let color = color_gradient.eval_continuous(normalized_vol);
                let rgb = RGBColor(color.r, color.g, color.b);
                
                // Draw a small rectangle for this point
                chart
                    .draw_series(std::iter::once(Rectangle::new(
                    [
                        (strike - 0.5 * strike_range / surface.strikes.len() as f64, time - 0.5 * time_range / times_to_expiration.len() as f64),
                        (strike + 0.5 * strike_range / surface.strikes.len() as f64, time + 0.5 * time_range / times_to_expiration.len() as f64),
                    ],
                    rgb.filled(),
                )))
                    .map_err(|e| OptionsError::Other(e.to_string()))?;
            }
        }
    }
    
    // Add a color bar
    let color_bar_width = 20;
    let color_bar_height = 400;
    let color_bar_x = 750;
    let color_bar_y = 100;
    
    for i in 0..color_bar_height {
        let normalized_pos = 1.0 - (i as f64 / color_bar_height as f64);
        let color = color_gradient.eval_continuous(normalized_pos);
        let rgb = RGBColor(color.r, color.g, color.b);
        
        root.draw(
            &Rectangle::new(
            [
                (color_bar_x, color_bar_y + i),
                (color_bar_x + color_bar_width, color_bar_y + i + 1),
            ],
            rgb.filled(),
        ))
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    }
    
    // Add labels for the color bar
    root
        .draw_text(
        &format!("{:.2}", vol_max),
        &TextStyle::from(("sans-serif", 12)).color(&BLACK),
        (color_bar_x + color_bar_width + 5, color_bar_y),
    )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    root
        .draw_text(
        &format!("{:.2}", vol_min),
        &TextStyle::from(("sans-serif", 12)).color(&BLACK),
        (color_bar_x + color_bar_width + 5, color_bar_y + color_bar_height),
    )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    root
        .draw_text(
        "IV",
        &TextStyle::from(("sans-serif", 12)).color(&BLACK),
        (color_bar_x + color_bar_width + 5, color_bar_y + color_bar_height / 2),
    )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    // Add a note about the date
    root
        .draw_text(
        &format!("Generated: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")),
        &TextStyle::from(("sans-serif", 15)).color(&BLACK),
        (10, 570),
    )
        .map_err(|e| OptionsError::Other(e.to_string()))?;
    
    root.present().map_err(|e| OptionsError::Other(e.to_string()))?;
    
    Ok(())
}
