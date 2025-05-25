use options_rs::config::Config;
use options_rs::error::Result;
use options_rs::models::SurfaceUpdate;
use options_rs::utils::minifb_surface::{stream_quotes, VolatilitySurfaceVisualizer, SURFACE_BUS};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    config.init_logging()?;

    info!("Starting MiniFB Volatility Surface Plotter");

    let args: Vec<String> = std::env::args().collect();
    let symbol = if args.len() > 1 {
        args[1].clone()
    } else {
        "AAPL".to_string()
    };

    info!("Using ticker symbol: {}", symbol);

    // Create and initialize the visualizer first
    let mut visualizer = match VolatilitySurfaceVisualizer::new(&symbol) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to create visualizer: {}", e);
            return Err(e);
        }
    };

    // Create an initial empty surface update to prevent the GUI from hanging
    // This ensures the GUI has something to display while waiting for real data
    let initial_update = SurfaceUpdate {
        strikes: vec![100.0, 200.0, 300.0, 400.0, 500.0],
        expiries: vec![chrono::Local::now().date_naive()],
        sigma: vec![0.0; 5], // 5 strikes × 1 expiry
    };

    // Send the initial update to the visualizer
    let _ = SURFACE_BUS.send(initial_update);

    // Now spawn the data feed in the background
    let alpaca_cfg = config.alpaca.clone();
    tokio::spawn(stream_quotes(symbol.clone(), alpaca_cfg.clone()));

    // Run the GUI - it will now have initial data to display
    match visualizer.run(alpaca_cfg) {
        Ok(_) => {
            info!("Visualizer exited normally");
            Ok(())
        }
        Err(e) => {
            warn!("Visualizer exited with error: {}", e);
            Err(e)
        }
    }
}
