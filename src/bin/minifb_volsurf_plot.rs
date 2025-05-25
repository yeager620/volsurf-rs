use options_rs::config::Config;
use options_rs::error::Result;
use options_rs::utils::minifb_plotting::VolatilitySurfaceVisualizer;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from environment variables
    let config = Config::from_env()?;
    config.init_logging()?;

    info!("Starting MiniFB Volatility Surface Plotter");

    // Get the ticker symbol from command line arguments
    let args: Vec<String> = std::env::args().collect();
    let symbol = if args.len() > 1 {
        args[1].clone()
    } else {
        // Default to SPY if no symbol is provided
        "SPY".to_string()
    };

    info!("Using ticker symbol: {}", symbol);

    // Create a new volatility surface visualizer
    let mut visualizer = match VolatilitySurfaceVisualizer::new(&symbol) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to create visualizer: {}", e);
            return Err(e);
        }
    };

    // Run the visualizer
    match visualizer.run(config.alpaca.clone()) {
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
