use crate::config::ETradeConfig;
use crate::error::{OptionsError, Result};
use crate::models::{OptionContract, OptionQuote, OptionType};
use chrono::{DateTime, NaiveDate, Utc, Datelike};
use hmac::{Hmac, Mac};
use rand::Rng;
use reqwest::RequestBuilder;
use serde::Deserialize;
use sha1::Sha1;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use percent_encoding::{percent_encode, NON_ALPHANUMERIC};

/// OAuth credentials required for signing requests
#[derive(Debug, Clone)]
struct OAuthCreds {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_secret: String,
}

impl OAuthCreds {
    fn new(cfg: &ETradeConfig) -> Self {
        Self {
            consumer_key: cfg.consumer_key.clone(),
            consumer_secret: cfg.consumer_secret.clone(),
            access_token: cfg.access_token.clone(),
            access_secret: cfg.access_secret.clone(),
        }
    }

    fn sign(&self, req: RequestBuilder, method: &str, url: &str, query: &[(String, String)]) -> Result<RequestBuilder> {
        let nonce: u64 = rand::thread_rng().gen();
        let timestamp = Utc::now().timestamp();

        let mut params: Vec<(String, String)> = Vec::new();
        params.push(("oauth_consumer_key".into(), self.consumer_key.clone()));
        params.push(("oauth_token".into(), self.access_token.clone()));
        params.push(("oauth_signature_method".into(), "HMAC-SHA1".into()));
        params.push(("oauth_timestamp".into(), timestamp.to_string()));
        params.push(("oauth_nonce".into(), nonce.to_string()));
        params.push(("oauth_version".into(), "1.0".into()));
        for (k, v) in query {
            params.push((k.clone(), v.clone()));
        }
        params.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut param_str = String::new();
        for (i, (k, v)) in params.iter().enumerate() {
            if i > 0 {
                param_str.push('&');
            }
            param_str.push_str(&format!("{}={}", percent_encode(k.as_bytes(), NON_ALPHANUMERIC), percent_encode(v.as_bytes(), NON_ALPHANUMERIC)));
        }

        let base = format!("{}&{}&{}", method.to_uppercase(), percent_encode(url.as_bytes(), NON_ALPHANUMERIC), percent_encode(param_str.as_bytes(), NON_ALPHANUMERIC));
        let key = format!("{}&{}", percent_encode(self.consumer_secret.as_bytes(), NON_ALPHANUMERIC), percent_encode(self.access_secret.as_bytes(), NON_ALPHANUMERIC));
        let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes()).map_err(|e| OptionsError::Other(e.to_string()))?;
        mac.update(base.as_bytes());
        let result = mac.finalize().into_bytes();
        let signature = BASE64.encode(result);

        let mut header_params = vec![
            ("oauth_consumer_key", self.consumer_key.as_str()),
            ("oauth_token", self.access_token.as_str()),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", &timestamp.to_string()),
            ("oauth_nonce", &nonce.to_string()),
            ("oauth_version", "1.0"),
            ("oauth_signature", &signature),
        ];
        let mut auth = String::from("OAuth ");
        for (i, (k, v)) in header_params.iter().enumerate() {
            if i > 0 {
                auth.push_str(", ");
            }
            auth.push_str(&format!("{}=\"{}\"", k, percent_encode(v.as_bytes(), NON_ALPHANUMERIC)));
        }

        Ok(req.header("Authorization", auth))
    }
}

#[derive(Clone)]
pub struct ETradeClient {
    http: reqwest::Client,
    creds: OAuthCreds,
    sandbox: bool,
}

impl ETradeClient {
    pub fn new(cfg: ETradeConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            creds: OAuthCreds::new(&cfg),
            sandbox: cfg.sandbox,
        }
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str, query: &[(String, String)]) -> Result<T> {
        let base = if self.sandbox { "https://apisb.etrade.com" } else { "https://api.etrade.com" };
        let url = format!("{}{}", base, path);
        let req = self.http.get(&url);
        let signed = self.creds.sign(req, "GET", &url, query)?;
        let mut req_with_query = signed;
        for (k, v) in query {
            req_with_query = req_with_query.query(&[(k, v)]);
        }
        let res = req_with_query.send().await.map_err(|e| OptionsError::Other(e.to_string()))?;
        let res = res.error_for_status().map_err(|e| OptionsError::Other(e.to_string()))?;
        Ok(res.json::<T>().await.map_err(|e| OptionsError::ParseError(e.to_string()))?)
    }

    pub async fn lookup(&self, search: &str) -> Result<Vec<LookupItem>> {
        let path = format!("/v1/market/lookup/{}", search);
        let query: Vec<(String, String)> = Vec::new();
        let resp: LookupResponse = self.get(&path, &query).await?;
        Ok(resp.company)
    }

    pub async fn option_expire_dates(&self, symbol: &str) -> Result<Vec<NaiveDate>> {
        let query = vec![
            ("symbol".to_string(), symbol.to_string()),
            ("expiryType".to_string(), "ALL".to_string()),
        ];
        let resp: ExpireDateResponse = self.get("/v1/market/optionexpiredate", &query).await?;
        Ok(resp.expiration_dates)
    }

    pub async fn option_chains(&self, symbol: &str, date: NaiveDate) -> Result<Vec<OptionQuote>> {
        let query = vec![
            ("symbol".to_string(), symbol.to_string()),
            ("expiryYear".to_string(), date.year().to_string()),
            ("expiryMonth".to_string(), date.month().to_string()),
            ("expiryDay".to_string(), date.day().to_string()),
            ("includeWeekly".to_string(), "true".to_string()),
        ];
        let resp: ChainsResponse = self.get("/v1/market/optionchains", &query).await?;
        let mut quotes = Vec::new();
        for pair in resp.option_pairs {
            let option_type = if pair.option_type == "CALL" { OptionType::Call } else { OptionType::Put };
            let contract = OptionContract::new(symbol.to_string(), option_type, pair.strike_price, DateTime::<Utc>::from_utc(date.and_hms_opt(16,0,0).unwrap(), Utc));
            let quote = OptionQuote {
                contract,
                bid: pair.bid.unwrap_or(0.0),
                ask: pair.ask.unwrap_or(0.0),
                last: pair.last_price.unwrap_or(0.0),
                volume: 0,
                open_interest: pair.open_interest.unwrap_or(0) as u64,
                underlying_price: 0.0,
                timestamp: Utc::now(),
            };
            quotes.push(quote);
        }
        Ok(quotes)
    }

    pub async fn quotes(&self, symbols: &[&str]) -> Result<Vec<UnderlyingQuote>> {
        let list = symbols.join(",");
        let path = format!("/v1/market/quote/{}", list);
        let query = vec![("detailFlag".to_string(), "ALL".to_string())];
        let resp: QuotesResponse = self.get(&path, &query).await?;
        Ok(resp.quotes)
    }
}

#[derive(Debug, Deserialize)]
struct LookupResponse {
    #[serde(default)]
    company: Vec<LookupItem>,
}

#[derive(Debug, Deserialize)]
pub struct LookupItem {
    pub symbol: String,
    pub security_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExpireDateResponse {
    #[serde(rename = "expiryDates")]
    expiration_dates: Vec<NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct ChainsResponse {
    #[serde(rename = "optionPairs")]
    option_pairs: Vec<OptionPair>,
}

#[derive(Debug, Deserialize)]
struct OptionPair {
    #[serde(rename = "optionType")]
    option_type: String,
    #[serde(rename = "strikePrice")]
    strike_price: f64,
    bid: Option<f64>,
    ask: Option<f64>,
    #[serde(rename = "lastPrice")]
    last_price: Option<f64>,
    #[serde(rename = "openInterest")]
    open_interest: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct QuotesResponse {
    #[serde(rename = "quoteData")]
    quotes: Vec<UnderlyingQuote>,
}

#[derive(Debug, Deserialize)]
pub struct UnderlyingQuote {
    pub symbol: String,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub lastTrade: Option<f64>,
    pub totalVolume: Option<u64>,
}
