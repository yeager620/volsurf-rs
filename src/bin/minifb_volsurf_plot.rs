use options_rs::config::Config;
use options_rs::error::Result;
use options_rs::utils::minifb_surface::{stream_quotes, VolatilitySurfaceVisualizer};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    config.init_logging()?;

    info!("Starting MiniFB Volatility Surface Plotter");

    let args: Vec<String> = std::env::args().collect();
    let symbol = if args.len() > 1 { args[1].clone() } else { "SPY".to_string() };

    info!("Using ticker symbol: {}", symbol);

    // spawn data feed
    let alpaca_cfg = config.alpaca.clone();
    tokio::spawn(stream_quotes(symbol.clone(), alpaca_cfg.clone()));

    // run gui
    let mut visualizer = match VolatilitySurfaceVisualizer::new(&symbol) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to create visualizer: {}", e);
            return Err(e);
        }
    };

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
