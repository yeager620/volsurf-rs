## Authentication & Base URLs

All REST endpoints require two HTTP headers for authentication:

* `APCA-API-KEY-ID`: your API key ID
* `APCA-API-SECRET-KEY`: your API secret ([Alpaca API Docs][1])

| Asset Class    | Base URL                                                                       |
| -------------- | ------------------------------------------------------------------------------ |
| Stocks         | `https://data.alpaca.markets/v2/stocks/...`             ([Alpaca API Docs][1]) |
| Options (BETA) | `https://data.alpaca.markets/v1beta1/options/...`  ([Alpaca API Docs][3])      |

---


## Stocks Market Data Endpoints

### Historical Bars

**GET** `/v2/stocks/bars`
**Params:** `symbols`, `timeframe`, `start`, `end`, `limit`, `feed`, `asof`, `page_token`, `sort` ([Alpaca API Docs][1], [Postman API Platform][2])
**Response:**

```json
{
  "bars": [
    {
      "t":"2025-05-24T09:30:00Z",  
      "o":100.0,                   
      "h":101.5,                   
      "l":99.7,                    
      "c":100.8,                   
      "v":120000,                  
      "n":350,                     
      "vw":100.45,                 
      "symbol":"AAPL"              
    },
    "..."  
  ],
  "next_page_token":"abcdef"     
}
```

### Latest Bars  
**GET** `/v2/stocks/bars/latest`  
**Params:** `symbols`, `feed`  
**Response:**  
```json
{
  "bars":{
    "AAPL":{
      "t":"2025-05-24T14:59:00Z","o":150.1,"h":150.5,"l":149.9,"c":150.2,"v":5000,"n":45,"vw":150.236
    },
    "TSLA":{
      "t":"2025-05-24T14:59:00Z","o":145.3,"h":145.8,"l":144.9,"c":145.5,"v":4200,"n":38,"vw":145.412
    }
  }
}
```

### Historical Trades  
**GET** `/v2/stocks/trades`  
**Params:** as above (minus `timeframe`)  
**Response objects:**  
```json
{
  "trades": [
    {
      "t":"2025-05-24T09:31:05Z", 
      "price":150.25,             
      "size":100,                 
      "conditions":["P","Z"],           
      "exchange_code":"N"         
    },
    "..."
  ],
  "next_page_token":null
}
```  

### Latest Trades  
**GET** `/v2/stocks/trades/latest`  
**Params:** `symbols`, `feed`  
Returns the most recent trade tick for each symbol.

### Historical Quotes  
**GET** `/v2/stocks/quotes`  
**Params:** same as trades  
**Fields:**  
- `t`: timestamp  
- `bidprice`, `bidsize`  
- `askprice`, `asksize`  
- `symbol` (with multi-symbol)  

### Latest Quotes  
**GET** `/v2/stocks/quotes/latest`  
**Params:** `symbols`, `feed`  
**Response:** map of symbol → latest quote object.

### Snapshots  
- **Multi-symbol:** **GET** `/v2/stocks/snapshots?symbols=AAPL,TSLA` returns for each symbol the latest trade, quote, minute bar, daily bar, and previous daily bar.  
- **Single-symbol:** **GET** `/v2/stocks/{symbol}/snapshot`.  

### Condition & Exchange Codes  
- **GET** `/v2/stocks/meta/conditions/:ticktype?tape=C` returns a map of condition code → description.  
- **GET** `/v2/stocks/meta/exchanges` returns a map of exchange code → exchange name.

---

## Options Market Data Endpoints (BETA)  

### Get Option Contracts  
**GET** `/v2/options/contracts`  
**Params:**  
- `underlying_symbols` (e.g. `AAPL`)  
- `expiration_date_lte`, `limit`, `offset`  
Returns a list of contract metadata (`symbol`, `id`, `strike_price`, `expiration_date`, `contract_type`, `multiplier`, `underlying_symbol`).

**Response:**
```json
{
  "results": [
    {
      "symbol": "AAPL240621C00200000",
      "id": "AAPL240621C00200000",
      "strike_price": 200.0,
      "expiration_date": "2024-06-21",
      "contract_type": "call",
      "multiplier": 100,
      "underlying_symbol": "AAPL"
    },
    {
      "symbol": "AAPL240621P00200000",
      "id": "AAPL240621P00200000",
      "strike_price": 200.0,
      "expiration_date": "2024-06-21",
      "contract_type": "put",
      "multiplier": 100,
      "underlying_symbol": "AAPL"
    }
  ],
  "next_page_token": "abc123"
}
```

### Option Chain  
**GET** `/v1beta1/options/snapshots/{underlying_symbol}`  
Returns latest trade, quote, and Greeks for *all* option contracts of the given underlying (calls and puts).  
**Greeks fields:** `delta`, `gamma`, `theta`, `vega`, `rho`.  

**Response:**
```json
{
  "results": [
    {
      "symbol": "AAPL240621C00200000",
      "underlying_symbol": "AAPL",
      "strike_price": 200.0,
      "expiration_date": "2024-06-21",
      "contract_type": "call",
      "last_trade": {
        "t": "2024-06-10T15:30:00Z",
        "price": 5.65,
        "size": 10,
        "conditions": ["@", "R"],
        "exchange_code": "A"
      },
      "last_quote": {
        "t": "2024-06-10T15:30:05Z",
        "bid": 5.60,
        "ask": 5.70,
        "size_bid": 5,
        "size_ask": 15
      },
      "greeks": {
        "delta": 0.45,
        "gamma": 0.05,
        "theta": -0.15,
        "vega": 0.25,
        "rho": 0.08
      }
    },
    {
      "symbol": "AAPL240621P00200000",
      "underlying_symbol": "AAPL",
      "strike_price": 200.0,
      "expiration_date": "2024-06-21",
      "contract_type": "put",
      "last_trade": {
        "t": "2024-06-10T15:29:45Z",
        "price": 4.25,
        "size": 5,
        "conditions": ["@"],
        "exchange_code": "C"
      },
      "last_quote": {
        "t": "2024-06-10T15:30:05Z",
        "bid": 4.20,
        "ask": 4.30,
        "size_bid": 10,
        "size_ask": 8
      },
      "greeks": {
        "delta": -0.55,
        "gamma": 0.05,
        "theta": -0.12,
        "vega": 0.23,
        "rho": -0.07
      }
    }
  ]
}
```

### Historical Bars
**GET** `/v1beta1/options/bars`
**Params:** `symbols`, `timeframe`, `start`, `end`, `limit`, `page_token`, `sort`
Response structure identical to stock bars but for option symbols.

**Response:**
```json
{
  "bars": {
    "AAPL240621C00200000": [
      {
        "t": "2024-06-10T09:30:00Z",
        "o": 5.50,
        "h": 5.75,
        "l": 5.40,
        "c": 5.65,
        "v": 1250,
        "n": 45,
        "vw": 5.58
      },
      {
        "t": "2024-06-10T09:31:00Z",
        "o": 5.65,
        "h": 5.80,
        "l": 5.60,
        "c": 5.75,
        "v": 980,
        "n": 32,
        "vw": 5.72
      }
    ]
  },
  "next_page_token": "xyz789"
}
```

### Latest Quotes  
**GET** `/v1beta1/options/quotes/latest`  
**Params:** `symbols`  
Returns latest bid/ask for each contract: `{ "t", "bid", "ask", "size_bid", "size_ask", "symbol" }`.

**Response:**
```json
{
  "quotes": {
    "AAPL240621C00200000": {
      "t": "2024-06-10T15:45:05Z",
      "bid": 5.60,
      "ask": 5.70,
      "size_bid": 5,
      "size_ask": 15,
      "symbol": "AAPL240621C00200000"
    },
    "AAPL240621P00200000": {
      "t": "2024-06-10T15:45:05Z",
      "bid": 4.20,
      "ask": 4.30,
      "size_bid": 10,
      "size_ask": 8,
      "symbol": "AAPL240621P00200000"
    }
  }
}
```

### Historical Trades
**GET** `/v1beta1/options/trades`
**Params:** `symbols`, `start`, `end`, `limit`, `page_token`, `sort`
Up to 7 days of option trade ticks with fields `{ "t", "price", "size", "conditions", "exchange_code" }`.

**Response:**
```json
{
  "trades": [
    {
      "t": "2024-06-10T15:30:00Z",
      "price": 5.65,
      "size": 10,
      "conditions": ["@", "R"],
      "exchange_code": "A",
      "symbol": "AAPL240621C00200000"
    },
    {
      "t": "2024-06-10T15:29:45Z",
      "price": 4.25,
      "size": 5,
      "conditions": ["@"],
      "exchange_code": "C",
      "symbol": "AAPL240621P00200000"
    }
  ],
  "next_page_token": "def456"
}
```

### Latest Trades
**GET** `/v1beta1/options/trades/latest`
**Params:** `symbols`
Returns most recent trade tick per contract symbol.

**Response:**
```json
{
  "trades": {
    "AAPL240621C00200000": {
      "t": "2024-06-10T15:30:00Z",
      "price": 5.65,
      "size": 10,
      "conditions": ["@", "R"],
      "exchange_code": "A"
    },
    "AAPL240621P00200000": {
      "t": "2024-06-10T15:29:45Z",
      "price": 4.25,
      "size": 5,
      "conditions": ["@"],
      "exchange_code": "C"
    }
  }
}
```

### Snapshots
- **Multi-symbol:** **GET** `/v1beta1/options/snapshots?symbols=…` returns latest trade, quote, and greeks per contract. Optional params: `feed`, `updated_since`, `limit`, `page_token`.
- **Underlying-chain:** covered by the Option Chain endpoint above.

**Response:**
```json
{
  "snapshots": {
    "AAPL240621C00200000": {
      "symbol": "AAPL240621C00200000",
      "underlying_symbol": "AAPL",
      "strike_price": 200.0,
      "expiration_date": "2024-06-21",
      "contract_type": "call",
      "last_trade": {
        "t": "2024-06-10T15:30:00Z",
        "price": 5.65,
        "size": 10,
        "conditions": ["@", "R"],
        "exchange_code": "A"
      },
      "last_quote": {
        "t": "2024-06-10T15:30:05Z",
        "bid": 5.60,
        "ask": 5.70,
        "size_bid": 5,
        "size_ask": 15
      },
      "greeks": {
        "delta": 0.45,
        "gamma": 0.05,
        "theta": -0.15,
        "vega": 0.25,
        "rho": 0.08
      }
    },
    "AAPL240621P00200000": {
      "symbol": "AAPL240621P00200000",
      "underlying_symbol": "AAPL",
      "strike_price": 200.0,
      "expiration_date": "2024-06-21",
      "contract_type": "put",
      "last_trade": {
        "t": "2024-06-10T15:29:45Z",
        "price": 4.25,
        "size": 5,
        "conditions": ["@"],
        "exchange_code": "C"
      },
      "last_quote": {
        "t": "2024-06-10T15:30:05Z",
        "bid": 4.20,
        "ask": 4.30,
        "size_bid": 10,
        "size_ask": 8
      },
      "greeks": {
        "delta": -0.55,
        "gamma": 0.05,
        "theta": -0.12,
        "vega": 0.23,
        "rho": -0.07
      }
    }
  }
}
```

### Condition & Exchange Codes
Analogous to stocks, the BETA metadata endpoints:
- **GET** `/v1beta1/options/meta/conditions/{ticktype}` (e.g. `/trade`)
- **GET** `/v1beta1/options/meta/exchanges`
return code → description maps.

**Response for Conditions:**
```json
{
  "A": "CANC - Transaction previously reported",
  "B": "OSEQ - Transaction is being reported late and is out of sequence",
  "C": "CNCL - Transaction is the last reported for the particular option contract and is now cancelled",
  "D": "LATE - Transaction is being reported late, but is in the correct sequence",
  "E": "CNCO - Transaction was the first one (opening) reported this day for the particular option contract and is now cancelled",
  "F": "OPEN - Transaction is a late report of the opening trade and is out of sequence",
  "G": "CNOL - Transaction was the only one reported this day for the particular option contract and is now to be cancelled",
  "H": "OPNL - Transaction is a late report of the opening trade, but is in the correct sequence",
  "I": "AUTO - Transaction was executed electronically",
  "J": "REOP - Transaction is a reopening of an option contract in which trading has been previously halted",
  "S": "ISOI - Transaction was the execution of an order identified as an Intermarket Sweep Order",
  "a": "SLAN - Single Leg Auction Non ISO",
  "b": "SLAI - Single Leg Auction ISO",
  "c": "SLCN - Single Leg Cross Non ISO",
  "d": "SCLI - Single Leg Cross ISO",
  "e": "SLFT - Single Leg Floor Trade",
  "f": "MLET - Multi Leg autoelectronic trade",
  "g": "MLAT - Multi Leg Auction",
  "h": "MLCT - Multi Leg Cross",
  "i": "MLFT - Multi Leg floor trade",
  "j": "MESL - Multi Leg autoelectronic trade against single leg(s)",
  "k": "TLAT - Stock Options Auction",
  "l": "MASL - Multi Leg Auction against single leg(s)",
  "m": "MFSL - Multi Leg floor trade against single leg(s)",
  "n": "TLET - Stock Options autoelectronic trade",
  "o": "TLCT - Stock Options Cross",
  "p": "TLFT - Stock Options floor trade",
  "q": "TESL - Stock Options autoelectronic trade against single leg(s)",
  "r": "TASL - Stock Options Auction against single leg(s)",
  "s": "TFSL - Stock Options floor trade against single leg(s)",
  "t": "CBMO - Multi Leg Floor Trade of Proprietary Products",
  "u": "MCTP - Multilateral Compression Trade of Proprietary Products",
  "v": "EXHT - Extended Hours Trade"
}
```

**Response for Exchanges:**
```json
{
  "A": "AMEX - NYSE American",
  "B": "BOX - Boston Options Exchange",
  "C": "CBOE - Cboe Options Exchange",
  "D": "EMERALD - Miami International Stock Exchange Emerald Options",
  "E": "EDGX - Cboe EDGX Options Exchange",
  "H": "GEMX - Nasdaq GEMX",
  "I": "ISE - Nasdaq International Securities Exchange",
  "J": "MRX - Nasdaq MRX",
  "M": "MIAX - Miami International Stock Exchange",
  "N": "NYSE - NYSE Arca",
  "O": "OPRA - Options Price Reporting Authority",
  "P": "PEARL - Miami International Stock Exchange Pearl Options",
  "Q": "NASD - Nasdaq Options",
  "S": "SPHR - Miami International Stock Exchange Sapphire Options",
  "T": "BX - Nasdaq BX Options",
  "U": "MEMX - Members Options Exchange",
  "W": "C2 - Cboe C2 Options Exchange",
  "X": "PHLX - Nasdaq PHLX",
  "Z": "BATS - Cboe BZX Options Exchange"
}
```

---

## Error Handling  
On failure, endpoints return an HTTP 4xx or 5xx status and a JSON body:  
```json
{ "code":400, "message":"Invalid symbol" }
```

Use `code` and `message` to diagnose client or server errors ([Postman API Platform][2]).

---

## Example Code in Rust

### Using the `alpaca_api_client` Crate

```rust
use alpaca_api_client::{Client, market_data::{BarsRequest, TimeFrame}};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("APCA_API_KEY_ID")?;
    let api_secret = std::env::var("APCA_API_SECRET_KEY")?;
    let client = Client::new(api_key, api_secret);

    // Fetch 1-day bars for AAPL and TSLA
    let req = BarsRequest::builder()
        .symbols(vec!["AAPL".into(), "TSLA".into()])
        .timeframe(TimeFrame::OneDay)
        .start("2025-05-01T00:00:00Z")
        .end("2025-05-24T00:00:00Z")
        .build();

    let resp = client.market_data().get_bars(req).await?;
    for bar in resp.bars {
        println!(
            "{} @ {}: O{} H{} L{} C{} V{}",
            bar.symbol, bar.t, bar.o, bar.h, bar.l, bar.c, bar.v
        );
    }
    Ok(())
}
```

— Rust SDK: `alpaca_api_client` on crates.io ([Crates.io][5], [Docs.rs][6])

### Raw HTTP with `reqwest`

```rust
use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize)]
struct Snapshot {
    symbol: String,
    last_trade: Trade,
    last_quote: Quote,
    greeks: Greeks,
}
#[derive(Deserialize)] struct Trade { t:String, price:f64, size:u64 }
#[derive(Deserialize)] struct Quote { t:String, bid:f64, ask:f64, size_bid:u64, size_ask:u64 }
#[derive(Deserialize)] struct Greeks { delta:f64, gamma:f64, theta:f64, vega:f64, rho:f64 }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = "https://data.alpaca.markets/v1beta1/options/snapshots";
    let resp = client.get(url)
        .header("APCA-API-KEY-ID", std::env::var("APCA_API_KEY_ID")?)
        .header("APCA-API-SECRET-KEY", std::env::var("APCA_API_SECRET_KEY")?)
        .query(&[("symbols","AAPL240617C00145000"),("symbols","AAPL240617P00145000")])
        .send().await?
        .json::<Vec<Snapshot>>().await?;
    println!("{:#?}", resp);
    Ok(())
}
```

— Raw HTTP approach inspired by GitHub’s unofficial Rust SDK ([github.com][7])

[1]: https://docs.alpaca.markets/reference/stockbars "Historical bars"
[2]: https://www.postman.com/alpacamarkets/alpaca-public-workspace/documentation/4bx4njh/market-data-v2-api?utm_source=chatgpt.com "Market Data v2 API | Documentation | Postman API Network"
[3]: https://docs.alpaca.markets/reference/optionsnapshots?utm_source=chatgpt.com "Snapshots - Alpaca API Docs"
[4]: https://docs.alpaca.markets/reference/optionchain?utm_source=chatgpt.com "Option chain - Alpaca API Docs"
[5]: https://crates.io/crates/alpaca_api_client?utm_source=chatgpt.com "alpaca_api_client - crates.io: Rust Package Registry"
[6]: https://docs.rs/alpaca_api_client?utm_source=chatgpt.com "alpaca_api_client - Rust - Docs.rs"
[7]: https://github.com/PassivityTrading/alpaca-rs?utm_source=chatgpt.com "PassivityTrading/alpaca-rs: A library for working with alpaca ... - GitHub"
