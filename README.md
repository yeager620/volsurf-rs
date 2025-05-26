# Options-RS

A Rust library for options pricing, volatility surface calculations, and market data processing using Alpaca Markets API.

## Features

- Low-latency Alpaca Markets API client for options data
- Options pricing and implied volatility calculations
- Volatility surface construction and visualization
- High-performance data processing with Polars
- Caching mechanisms for improved performance

## Polars Integration

This project uses the [Polars](https://github.com/pola-rs/polars) crate for high-performance data processing. Polars provides:

1. **Columnar Storage & Arrow Under the Hood**  
   Polars is built on Apache Arrow's columnar memory model, so operations like filtering, aggregation, and joins are SIMD-friendly and multi-threaded by default.

2. **Lazy API & Query Optimization**  
   The Lazy API builds up a pipeline of operations that are optimized before execution. The optimizer fuses operations, pushes down predicates, and parallelizes scans.

3. **In-Memory Caching**  
   The `.cache()` method pins the result of a computation in memory so repeated queries don't re-run the entire pipeline.

4. **Persistent On-Disk Caching**  
   Parquet and IPC files are used for efficient on-disk caching, with memory-mapping and zero-copy where possible.

5. **Parallel Execution**  
   Polars automatically parallelizes operations like groupby, join, filter, etc.

## Usage

### Converting Option Quotes to DataFrames

```rust
use options_rs::utils::polars_utils;
use options_rs::models::OptionQuote;

// Assuming you have a vector of OptionQuote objects
let quotes: Vec<OptionQuote> = /* ... */;

// Convert to DataFrame
let df = polars_utils::quotes_to_dataframe(&quotes)?;

// Process with Lazy API
let result = df.lazy()
    .filter(col("bid").gt(lit(0.0)))
    .with_columns([
        ((col("bid") + col("ask")) / lit(2.0)).alias("mid_price")
    ])
    .collect()?;
```

### Calculating Volatility Surface with Polars

```rust
use options_rs::utils::polars_utils;

// Calculate volatility surface with Polars
let risk_free_rate = 0.03;
let surface = polars_utils::calculate_volatility_surface_with_polars(
    &quotes, 
    "AAPL", 
    risk_free_rate
)?;
```

### Caching to Disk

```rust
use options_rs::utils::polars_utils;

// Cache DataFrame to Parquet
let cache_file = "cache/data.parquet";
polars_utils::cache_dataframe_to_parquet(&df, cache_file)?;

// Load from cache
let cached_df = polars_utils::load_dataframe_from_parquet(cache_file)?;
```

## Project Tree
```
├── Cargo.lock
├── Cargo.toml
├── README.md
└── src
    ├── api
    │   ├── alpaca.md
    │   ├── mod.rs
    │   ├── rest.rs
    │   └── websocket.rs
    ├── bin
    │   ├── live_volsurf_plot.rs
    │   └── test_websocket.rs
    ├── config.rs
    ├── error.rs
    ├── lib.rs
    ├── models
    │   ├── mod.rs
    │   ├── option.rs
    │   └── volatility.rs
    └── utils
        ├── black_scholes.rs
        ├── minifb_plotting.rs
        ├── minifb_surface.rs
        ├── mod.rs
        ├── plotting.rs
        └── polars_utils.rs
```

## Performance Benefits

Using Polars provides significant performance improvements:

1. **Faster Data Processing**: Columnar storage and SIMD operations make filtering, aggregation, and joins much faster.
2. **Reduced Memory Usage**: Columnar format is more memory-efficient for analytical workloads.
3. **Automatic Parallelism**: Operations are automatically parallelized across available CPU cores.
4. **Caching**: Both in-memory and on-disk caching reduce redundant computations.
5. **Query Optimization**: The Lazy API optimizes query plans for maximum efficiency.

For large datasets or computation-intensive operations like volatility surface calculation, you can expect 5×–10× speedups compared to row-by-row processing.