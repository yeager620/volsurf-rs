use options_rs::api::WebSocketClient;
use options_rs::config::Config;
use options_rs::error::Result;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from environment variables
    let config = Config::from_env()?;
    config.init_logging()?;

    // Create WebSocket client
    let client = WebSocketClient::new(config.alpaca);

    // Connect to WebSocket with some option symbols
    // Replace these with actual option symbols
    let symbols = vec![
        "AAPL230616C00180000".to_string(),
        "AAPL230616P00180000".to_string(),
    ];
    client.connect(symbols).await?;

    // Process option quotes
    println!("Waiting for option quotes...");
    let mut count = 0;
    let start = std::time::Instant::now();

    // Process quotes for 10 seconds
    while start.elapsed().as_secs() < 10 {
        if let Some(quote) = client.next_option_quote().await? {
            count += 1;
            println!(
                "Received quote: {} - Bid: {}, Ask: {}, Mid: {}",
                quote.contract.option_symbol, quote.bid, quote.ask, quote.mid_price()
            );
        }
        sleep(Duration::from_millis(10)).await;
    }

    println!("Received {} quotes in 10 seconds", count);
    println!("Average: {} quotes per second", count as f64 / 10.0);

    Ok(())
}