pub mod api;
pub mod config;
pub mod error;
pub mod models;
pub mod utils;

pub use api::{RestClient, WebSocketClient};
pub use config::Config;
pub use error::{OptionsError, Result};

use models::{volatility::VolatilitySurface, OptionContract};
use chrono::Utc;

/// Fetch the option chain for a ticker using the REST client built from env config.
pub async fn fetch_chain(ticker: &str) -> Result<Vec<OptionContract>> {
    let config = Config::from_env()?;
    let rest = RestClient::new(config.alpaca.clone());
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let chain = rest
        .get_options_chain(
            ticker,
            None,
            Some(&today),
            None,
            None,
            None,
            Some(10000),
            None,
        )
        .await?;
    let mut contracts = Vec::new();
    for c in &chain.results {
        if let Some(contract) = OptionContract::from_occ_symbol(&c.symbol) {
            contracts.push(contract);
        }
    }
    Ok(contracts)
}

/// Build call and put volatility surfaces from a list of contracts.
pub fn build_surfaces(
    contracts: &[OptionContract],
    risk_free: f64,
) -> Result<(VolatilitySurface, VolatilitySurface)> {
    // TODO: fetch option quotes and compute separate call/put surfaces
    let quotes: Vec<api::rest::OptionQuote> = Vec::new();
    let call_surface = utils::polars_utils::calculate_volatility_surface_with_polars(&quotes, &contracts[0].symbol, risk_free)?;
    let put_surface = call_surface.clone();
    Ok((call_surface, put_surface))
}
