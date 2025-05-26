use crate::models::{OptionContract, OptionQuote, OptionType};
use crate::models::volatility::{ImpliedVolatility, VolatilitySurface};
use crate::error::{OptionsError, Result};
use chrono::{DateTime, Utc};
use polars::prelude::*;
use std::path::Path;

/// Convert a vector of OptionQuote to a Polars DataFrame
pub fn quotes_to_dataframe(quotes: &[OptionQuote]) -> Result<DataFrame> {
    if quotes.is_empty() {
        return Err(OptionsError::Other("Cannot create DataFrame from empty quotes".to_string()));
    }

    // Extract data into column vectors
    let mut symbols = Vec::with_capacity(quotes.len());
    let mut option_symbols = Vec::with_capacity(quotes.len());
    let mut option_types = Vec::with_capacity(quotes.len());
    let mut strikes = Vec::with_capacity(quotes.len());
    let mut expirations = Vec::with_capacity(quotes.len());
    let mut bids = Vec::with_capacity(quotes.len());
    let mut asks = Vec::with_capacity(quotes.len());
    let mut last_prices = Vec::with_capacity(quotes.len());
    let mut volumes = Vec::with_capacity(quotes.len());
    let mut open_interests = Vec::with_capacity(quotes.len());
    let mut underlying_prices = Vec::with_capacity(quotes.len());
    let mut timestamps = Vec::with_capacity(quotes.len());

    for quote in quotes {
        symbols.push(quote.contract.symbol.clone());
        option_symbols.push(quote.contract.option_symbol.clone());
        option_types.push(if quote.contract.is_call() { "Call" } else { "Put" });
        strikes.push(quote.contract.strike);
        expirations.push(quote.contract.expiration.timestamp_millis());
        bids.push(quote.bid);
        asks.push(quote.ask);
        last_prices.push(quote.last);
        volumes.push(quote.volume as i64);
        open_interests.push(quote.open_interest as i64);
        underlying_prices.push(quote.underlying_price);
        timestamps.push(quote.timestamp.timestamp_millis());
    }

    // Create Series for each column
    let df = DataFrame::new(vec![
        Series::new("symbol", symbols),
        Series::new("option_symbol", option_symbols),
        Series::new("option_type", option_types),
        Series::new("strike", strikes),
        Series::new("expiration", expirations),
        Series::new("bid", bids),
        Series::new("ask", asks),
        Series::new("last", last_prices),
        Series::new("volume", volumes),
        Series::new("open_interest", open_interests),
        Series::new("underlying_price", underlying_prices),
        Series::new("timestamp", timestamps),
    ])
    .map_err(|e| OptionsError::Other(format!("Failed to create DataFrame: {}", e)))?;

    Ok(df)
}

/// Convert a Polars DataFrame back to a vector of OptionQuote
pub fn dataframe_to_quotes(df: &DataFrame) -> Result<Vec<OptionQuote>> {
    let n_rows = df.height();
    let mut quotes = Vec::with_capacity(n_rows);

    // Get column references
    let symbols = df.column("symbol")?;
    let option_symbols = df.column("option_symbol")?;
    let option_types = df.column("option_type")?;
    let strikes = df.column("strike")?;
    let expirations = df.column("expiration")?;
    let bids = df.column("bid")?;
    let asks = df.column("ask")?;
    let lasts = df.column("last")?;
    let volumes = df.column("volume")?;
    let open_interests = df.column("open_interest")?;
    let underlying_prices = df.column("underlying_price")?;
    let timestamps = df.column("timestamp")?;

    for i in 0..n_rows {
        let symbol = symbols.utf8()?.get(i).unwrap_or("").to_string();
        let option_symbol = option_symbols.utf8()?.get(i).unwrap_or("").to_string();
        let option_type = if option_types.utf8()?.get(i).unwrap_or("") == "Call" {
            OptionType::Call
        } else {
            OptionType::Put
        };
        let strike = strikes.f64()?.get(i).unwrap_or(0.0);
        let expiration_millis = expirations.i64()?.get(i).unwrap_or(0);
        let expiration = DateTime::<Utc>::from_timestamp_millis(expiration_millis)
            .ok_or_else(|| OptionsError::Other("Invalid expiration timestamp".to_string()))?;

        let bid = bids.f64()?.get(i).unwrap_or(0.0);
        let ask = asks.f64()?.get(i).unwrap_or(0.0);
        let last = lasts.f64()?.get(i).unwrap_or(0.0);
        let volume = volumes.i64()?.get(i).unwrap_or(0) as u64;
        let open_interest = open_interests.i64()?.get(i).unwrap_or(0) as u64;
        let underlying_price = underlying_prices.f64()?.get(i).unwrap_or(0.0);
        let timestamp_millis = timestamps.i64()?.get(i).unwrap_or(0);
        let timestamp = DateTime::<Utc>::from_timestamp_millis(timestamp_millis)
            .ok_or_else(|| OptionsError::Other("Invalid timestamp".to_string()))?;

        // Create contract
        let contract = OptionContract {
            symbol,
            option_type,
            strike,
            expiration,
            option_symbol,
        };

        // Create quote
        let quote = OptionQuote {
            contract,
            bid,
            ask,
            last,
            volume,
            open_interest,
            underlying_price,
            timestamp,
        };

        quotes.push(quote);
    }

    Ok(quotes)
}

/// Convert a vector of ImpliedVolatility to a Polars DataFrame
pub fn implied_volatilities_to_dataframe(ivs: &[ImpliedVolatility]) -> Result<DataFrame> {
    if ivs.is_empty() {
        return Err(OptionsError::Other("Cannot create DataFrame from empty implied volatilities".to_string()));
    }

    // Extract data into column vectors
    let mut symbols = Vec::with_capacity(ivs.len());
    let mut option_symbols = Vec::with_capacity(ivs.len());
    let mut option_types = Vec::with_capacity(ivs.len());
    let mut strikes = Vec::with_capacity(ivs.len());
    let mut expirations = Vec::with_capacity(ivs.len());
    let mut values = Vec::with_capacity(ivs.len());
    let mut underlying_prices = Vec::with_capacity(ivs.len());
    let mut option_prices = Vec::with_capacity(ivs.len());
    let mut times_to_expiration = Vec::with_capacity(ivs.len());
    let mut deltas = Vec::with_capacity(ivs.len());
    let mut vegas = Vec::with_capacity(ivs.len());

    for iv in ivs {
        symbols.push(iv.contract.symbol.clone());
        option_symbols.push(iv.contract.option_symbol.clone());
        option_types.push(if iv.contract.is_call() { "Call" } else { "Put" });
        strikes.push(iv.contract.strike);
        expirations.push(iv.contract.expiration.timestamp_millis());
        values.push(iv.value);
        underlying_prices.push(iv.underlying_price);
        option_prices.push(iv.option_price);
        times_to_expiration.push(iv.time_to_expiration);
        deltas.push(iv.delta);
        vegas.push(iv.vega);
    }

    // Create Series for each column
    let df = DataFrame::new(vec![
        Series::new("symbol", symbols),
        Series::new("option_symbol", option_symbols),
        Series::new("option_type", option_types),
        Series::new("strike", strikes),
        Series::new("expiration", expirations),
        Series::new("value", values),
        Series::new("underlying_price", underlying_prices),
        Series::new("option_price", option_prices),
        Series::new("time_to_expiration", times_to_expiration),
        Series::new("delta", deltas),
        Series::new("vega", vegas),
    ])
    .map_err(|e| OptionsError::Other(format!("Failed to create DataFrame: {}", e)))?;

    Ok(df)
}

/// Convert a VolatilitySurface to a Polars DataFrame
pub fn volatility_surface_to_dataframe(surface: &VolatilitySurface) -> Result<DataFrame> {
    let n_expirations = surface.expirations.len();
    let n_strikes = surface.strikes.len();
    let total_rows = n_expirations * n_strikes;

    // Create column vectors
    let mut expirations = Vec::with_capacity(total_rows);
    let mut strikes = Vec::with_capacity(total_rows);
    let mut volatilities = Vec::with_capacity(total_rows);

    // Flatten the 2D volatility surface into a long-format DataFrame
    for (i, &expiration) in surface.expirations.iter().enumerate() {
        for (j, &strike) in surface.strikes.iter().enumerate() {
            expirations.push(expiration.timestamp_millis());
            strikes.push(strike);
            volatilities.push(surface.volatilities[[i, j]]);
        }
    }

    // Create DataFrame
    let df = DataFrame::new(vec![
        Series::new("expiration", expirations),
        Series::new("strike", strikes),
        Series::new("volatility", volatilities),
    ])
    .map_err(|e| OptionsError::Other(format!("Failed to create DataFrame: {}", e)))?;

    Ok(df)
}

/// Create a VolatilitySurface from a Polars DataFrame
pub fn dataframe_to_volatility_surface(df: &DataFrame, symbol: &str) -> Result<VolatilitySurface> {
    // Get unique expirations and strikes
    let expirations_series = df.column("expiration")
        .map_err(|e| OptionsError::Other(format!("Failed to get 'expiration' column: {}", e)))?;
    let strikes_series = df.column("strike")
        .map_err(|e| OptionsError::Other(format!("Failed to get 'strike' column: {}", e)))?;

    let unique_expirations = expirations_series.unique()
        .map_err(|e| OptionsError::Other(format!("Failed to get unique expirations: {}", e)))?;
    let unique_strikes = strikes_series.unique()
        .map_err(|e| OptionsError::Other(format!("Failed to get unique strikes: {}", e)))?;

    let n_expirations = unique_expirations.len();
    let n_strikes = unique_strikes.len();

    // Convert to vectors
    let mut expirations = Vec::with_capacity(n_expirations);
    for i in 0..n_expirations {
        let millis = unique_expirations.i64()?.get(i).unwrap_or(0);
        let dt = DateTime::<Utc>::from_timestamp_millis(millis)
            .ok_or_else(|| OptionsError::Other("Invalid expiration timestamp".to_string()))?;
        expirations.push(dt);
    }
    expirations.sort();

    let mut strikes = Vec::with_capacity(n_strikes);
    for i in 0..n_strikes {
        let strike = unique_strikes.f64()?.get(i).unwrap_or(0.0);
        strikes.push(strike);
    }
    strikes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less));

    // Create volatility matrix
    let mut volatilities = ndarray::Array2::from_elem((n_expirations, n_strikes), f64::NAN);

    // Fill in volatility values
    let volatility_series = df.column("volatility")
        .map_err(|e| OptionsError::Other(format!("Failed to get 'volatility' column: {}", e)))?;

    for i in 0..df.height() {
        let exp_millis = expirations_series.i64()?.get(i).unwrap_or(0);
        let exp = DateTime::<Utc>::from_timestamp_millis(exp_millis)
            .ok_or_else(|| OptionsError::Other("Invalid expiration timestamp".to_string()))?;
        let strike = strikes_series.f64()?.get(i).unwrap_or(0.0);
        let volatility = volatility_series.f64()?.get(i).unwrap_or(f64::NAN);

        if let (Some(exp_idx), Some(strike_idx)) = (
            expirations.iter().position(|&e| e == exp),
            strikes.iter().position(|&s| s == strike),
        ) {
            volatilities[[exp_idx, strike_idx]] = volatility;
        }
    }

    Ok(VolatilitySurface {
        symbol: symbol.to_string(),
        expirations,
        strikes,
        volatilities,
        timestamp: Utc::now(),
        version: 1,
    })
}

/// Cache a DataFrame to disk in Parquet format
pub fn cache_dataframe_to_parquet(df: &DataFrame, path: &str) -> Result<()> {
    let file = std::fs::File::create(path)
        .map_err(|e| OptionsError::Other(format!("Failed to create file: {}", e)))?;

    let mut df_mut = df.clone();
    ParquetWriter::new(file)
        .finish(&mut df_mut)
        .map_err(|e| OptionsError::Other(format!("Failed to write Parquet file: {}", e)))?;

    Ok(())
}

/// Load a cached DataFrame from a Parquet file
pub fn load_dataframe_from_parquet(path: &str) -> Result<DataFrame> {
    if !Path::new(path).exists() {
        return Err(OptionsError::Other(format!("Parquet file not found: {}", path)));
    }

    let df = LazyFrame::scan_parquet(path, Default::default())
        .map_err(|e| OptionsError::Other(format!("Failed to scan Parquet file: {}", e)))?
        .collect()
        .map_err(|e| OptionsError::Other(format!("Failed to collect DataFrame: {}", e)))?;

    Ok(df)
}

/// Cache a DataFrame to disk in Arrow IPC format
pub fn cache_dataframe_to_ipc(df: &DataFrame, path: &str) -> Result<()> {
    let file = std::fs::File::create(path)
        .map_err(|e| OptionsError::Other(format!("Failed to create file: {}", e)))?;

    let mut df_mut = df.clone();
    IpcWriter::new(file)
        .finish(&mut df_mut)
        .map_err(|e| OptionsError::Other(format!("Failed to write IPC file: {}", e)))?;

    Ok(())
}

/// Load a cached DataFrame from an Arrow IPC file
pub fn load_dataframe_from_ipc(path: &str) -> Result<DataFrame> {
    if !Path::new(path).exists() {
        return Err(OptionsError::Other(format!("IPC file not found: {}", path)));
    }

    let df = LazyFrame::scan_ipc(path, Default::default())
        .map_err(|e| OptionsError::Other(format!("Failed to scan IPC file: {}", e)))?
        .collect()
        .map_err(|e| OptionsError::Other(format!("Failed to collect DataFrame: {}", e)))?;

    Ok(df)
}

/// Process option quotes using Polars' Lazy API for optimized performance
/// This function demonstrates how to use the Lazy API for query optimization
pub fn process_quotes_lazy(quotes: &[OptionQuote]) -> Result<DataFrame> {
    // Convert quotes to DataFrame
    let df = quotes_to_dataframe(quotes)?;

    // Create a LazyFrame from the DataFrame
    let lf = df.lazy();

    // Example of a complex query pipeline that benefits from Polars' query optimization
    let result = lf
        // Filter for quotes with non-zero bid and ask
        .filter(
            col("bid").gt(lit(0.0))
            .and(col("ask").gt(lit(0.0)))
        )
        // Add calculated columns
        .with_columns([
            ((col("bid") + col("ask")) / lit(2.0)).alias("mid_price"),
            (col("ask") - col("bid")).alias("spread"),
            ((col("ask") - col("bid")) / ((col("bid") + col("ask")) / lit(2.0))).alias("spread_pct"),
        ])
        // Group by symbol and option type
        .group_by([col("symbol"), col("option_type")])
        .agg([
            col("strike").mean().alias("avg_strike"),
            col("mid_price").mean().alias("avg_mid_price"),
            col("spread").mean().alias("avg_spread"),
            col("spread_pct").mean().alias("avg_spread_pct"),
            col("volume").sum().alias("total_volume"),
            col("option_symbol").count().alias("num_contracts"),
        ])
        // Sort by symbol and option type
        .sort_by_exprs(vec![col("symbol"), col("option_type")], vec![false, false], false, false)
        // Cache the result to avoid recomputation
        .cache();

    // Execute the query and materialize the result
    let result_df = result
        .collect()
        .map_err(|e| OptionsError::Other(format!("Failed to execute lazy query: {}", e)))?;

    Ok(result_df)
}

/// Calculate implied volatility surface using Polars for performance
pub fn calculate_volatility_surface_with_polars(
    quotes: &[OptionQuote], 
    symbol: &str, 
    risk_free_rate: f64
) -> Result<VolatilitySurface> {
    // Convert quotes to DataFrame
    let df = quotes_to_dataframe(quotes)?;

    // Create a LazyFrame from the DataFrame
    let lf = df.lazy();

    // Filter for valid quotes (non-zero bid and ask)
    let filtered_lf = lf
        .filter(col("bid").gt(lit(0.0)).and(col("ask").gt(lit(0.0))))
        .with_columns([
            ((col("bid") + col("ask")) / lit(2.0)).alias("mid_price"),
            ((col("ask") - col("bid")) / col("mid_price")).alias("spread_pct"),
        ])
        .filter(col("spread_pct").lt(lit(0.05)))
        .filter(col("volume").gt_eq(lit(10i64)).and(col("open_interest").gt_eq(lit(10i64))));

    // Materialize the filtered DataFrame
    let filtered_df = filtered_lf
        .collect()
        .map_err(|e| OptionsError::Other(format!("Failed to filter quotes: {}", e)))?;

    // Convert back to quotes for IV calculation
    let filtered_quotes = dataframe_to_quotes(&filtered_df)?;

    // Calculate implied volatilities
    let mut ivs = Vec::new();
    for q in &filtered_quotes {
        if let Ok(iv) = ImpliedVolatility::from_quote(q, risk_free_rate, 0.0) {
            ivs.push(iv);
        }
    }

    if ivs.is_empty() {
        return Err(OptionsError::VolatilityError(
            "No implied volatilities calculated".to_string(),
        ));
    }

    // Convert IVs to DataFrame for efficient processing
    let ivs_df = implied_volatilities_to_dataframe(&ivs)?;

    // Create volatility surface
    let surface = VolatilitySurface::new(symbol.to_string(), &ivs)?;

    Ok(surface)
}
