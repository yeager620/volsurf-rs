/// E*TRADE API client implementation with OAuth 1.0a authentication
///
/// This module provides a client for the E*TRADE API that handles OAuth 1.0a authentication,
/// token renewal, and error handling for 401 Unauthorized responses.
///
/// # OAuth 1.0a Flow
///
/// The E*TRADE API uses OAuth 1.0a for authentication. The flow is as follows:
///
/// 1. Get a request token using `ETradeClient::get_request_token()`
/// 2. Redirect the user to the authorization URL using `ETradeClient::get_authorize_url(request_token)`
/// 3. The user authorizes the application and gets a verification code
/// 4. Exchange the request token and verification code for an access token using
///    `ETradeClient::get_access_token(request_token, request_token_secret, verifier)`
///
/// # Token Expiration and Renewal
///
/// E*TRADE access tokens expire in two cases:
/// - After 2 hours of inactivity
/// - At midnight ET each day
///
/// This client automatically handles token renewal when a 401 Unauthorized response is received.
/// You can also manually renew the token using `ETradeClient::renew_access_token()`.
///
/// # Example
///
/// ```rust,no_run
/// use options_rs::api::ETradeClient;
/// use options_rs::config::Config;
///
/// async fn authenticate() {
///     let config = Config::from_env().unwrap();
///     let client = ETradeClient::new(config.etrade.clone());
///
///     // Check if we already have a valid token
///     if !client.has_valid_token() {
///         // Step 1: Get a request token
///         let (request_token, request_token_secret) = client.get_request_token().await.unwrap();
///
///         // Step 2: Get the authorization URL and redirect the user
///         let auth_url = client.get_authorize_url(&request_token);
///         println!("Please visit this URL to authorize the application: {}", auth_url);
///         println!("Enter the verification code:");
///
///         // Step 3: Get the verification code from the user
///         let mut verifier = String::new();
///         std::io::stdin().read_line(&mut verifier).unwrap();
///         let verifier = verifier.trim();
///
///         // Step 4: Exchange the request token for an access token
///         let (access_token, access_token_secret) = client
///             .get_access_token(&request_token, &request_token_secret, verifier)
///             .await
///             .unwrap();
///
///         println!("Successfully authenticated!");
///     }
///
///     // Now you can make API calls
///     let expiry_dates = client.option_expire_dates("AAPL").await.unwrap();
///     println!("AAPL expiry dates: {:?}", expiry_dates);
/// }
/// ```
use crate::config::ETradeConfig;
use crate::error::{OptionsError, Result};
use crate::models::{OptionContract, OptionQuote, OptionType};
use chrono::{NaiveDate, Utc, Datelike, TimeZone, DateTime};
use hmac::{Hmac, Mac};
use rand::Rng;
use reqwest::{RequestBuilder, StatusCode};
use serde::Deserialize;
use sha1::Sha1;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use percent_encoding::{percent_encode, NON_ALPHANUMERIC};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tracing::{debug, warn};

/// OAuth token data with expiration tracking
#[derive(Debug, Clone)]
struct OAuthToken {
    token: String,
    secret: String,
    created_at: SystemTime,
    last_used: SystemTime,
}

impl OAuthToken {
    fn new(token: String, secret: String) -> Self {
        let now = SystemTime::now();
        Self {
            token,
            secret,
            created_at: now,
            last_used: now,
        }
    }

    /// Check if token is expired (midnight ET or 2h inactivity)
    fn is_expired(&self) -> bool {
        let now = SystemTime::now();

        // Check if token has been inactive for more than 2 hours
        if let Ok(idle_duration) = now.duration_since(self.last_used) {
            if idle_duration > Duration::from_secs(2 * 60 * 60) {
                debug!("Token expired due to 2h inactivity");
                return true;
            }
        }

        // Check if it's past midnight ET
        let now_utc = Utc::now();
        let et_offset = chrono::FixedOffset::west_opt(5 * 60 * 60).unwrap(); // ET is UTC-5
        let now_et = now_utc.with_timezone(&et_offset);
        let token_created_et = DateTime::<Utc>::from(self.created_at).with_timezone(&et_offset);

        // If the current ET date is different from the token creation date, it's expired
        if now_et.date_naive() != token_created_et.date_naive() {
            debug!("Token expired due to midnight ET rollover");
            return true;
        }

        false
    }

    fn update_last_used(&mut self) {
        self.last_used = SystemTime::now();
    }
}

/// OAuth credentials required for signing requests
#[derive(Debug, Clone)]
struct OAuthCreds {
    consumer_key: String,
    consumer_secret: String,
    token: Arc<Mutex<Option<OAuthToken>>>,
    sandbox: bool,
    http_client: reqwest::Client,
}

impl OAuthCreds {
    fn new(cfg: &ETradeConfig, http_client: reqwest::Client) -> Self {
        let token = if !cfg.access_token.is_empty() && !cfg.access_secret.is_empty() {
            Some(OAuthToken::new(cfg.access_token.clone(), cfg.access_secret.clone()))
        } else {
            None
        };

        Self {
            consumer_key: cfg.consumer_key.clone(),
            consumer_secret: cfg.consumer_secret.clone(),
            token: Arc::new(Mutex::new(token)),
            sandbox: cfg.sandbox,
            http_client,
        }
    }

    /// Get the base URL for E*TRADE API
    fn base_url(&self) -> String {
        if self.sandbox {
            "https://apisb.etrade.com".to_string()
        } else {
            "https://api.etrade.com".to_string()
        }
    }

    /// Get request token (step 1 of OAuth flow)
    async fn get_request_token(&self) -> Result<(String, String)> {
        debug!("Getting OAuth request token");
        let callback = "oob"; // Out-of-band callback
        let url = format!("{}/oauth/request_token", self.base_url());

        let nonce: u64 = rand::thread_rng().gen();
        let timestamp = Utc::now().timestamp();
        let timestamp_str = timestamp.to_string();
        let nonce_str = nonce.to_string();

        // Create OAuth parameters
        let mut params = vec![
            ("oauth_consumer_key", self.consumer_key.as_str()),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", &timestamp_str),
            ("oauth_nonce", &nonce_str),
            ("oauth_version", "1.0"),
            ("oauth_callback", callback),
        ];

        // Sort parameters
        params.sort_by(|a, b| a.0.cmp(&b.0));

        // Create parameter string
        let param_str = params.iter()
            .map(|(k, v)| format!("{}={}", 
                percent_encode(k.as_bytes(), NON_ALPHANUMERIC),
                percent_encode(v.as_bytes(), NON_ALPHANUMERIC)))
            .collect::<Vec<String>>()
            .join("&");

        // Create signature base string
        let base = format!("{}&{}&{}", 
            "GET", 
            percent_encode(url.as_bytes(), NON_ALPHANUMERIC),
            percent_encode(param_str.as_bytes(), NON_ALPHANUMERIC));

        // Create signing key
        let key = format!("{}&", percent_encode(self.consumer_secret.as_bytes(), NON_ALPHANUMERIC));

        // Generate signature
        let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes())
            .map_err(|e| OptionsError::Other(e.to_string()))?;
        mac.update(base.as_bytes());
        let result = mac.finalize().into_bytes();
        let signature = BASE64.encode(result);

        // Add signature to parameters
        params.push(("oauth_signature", &signature));

        // Create Authorization header
        let auth_header = params.iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, percent_encode(v.as_bytes(), NON_ALPHANUMERIC)))
            .collect::<Vec<String>>()
            .join(", ");

        // Send request
        let response = self.http_client.get(&url)
            .header("Authorization", format!("OAuth {}", auth_header))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get request token: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_else(|_| "No response body".to_string());
            return Err(OptionsError::Other(format!("Failed to get request token: HTTP {} - {}", status, text)));
        }

        // Parse response
        let body = response.text().await
            .map_err(|e| OptionsError::ParseError(format!("Failed to read response: {}", e)))?;

        // Parse oauth_token and oauth_token_secret from response
        let params: Vec<(String, String)> = body.split('&')
            .filter_map(|pair| {
                let mut parts = pair.split('=');
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    Some((key.to_string(), value.to_string()))
                } else {
                    None
                }
            })
            .collect();

        let token = params.iter()
            .find(|(k, _)| k == "oauth_token")
            .map(|(_, v)| v.clone())
            .ok_or_else(|| OptionsError::ParseError("oauth_token not found in response".to_string()))?;

        let token_secret = params.iter()
            .find(|(k, _)| k == "oauth_token_secret")
            .map(|(_, v)| v.clone())
            .ok_or_else(|| OptionsError::ParseError("oauth_token_secret not found in response".to_string()))?;

        debug!("Got request token: {}", token);
        Ok((token, token_secret))
    }

    /// Get authorization URL (step 2 of OAuth flow)
    fn get_authorize_url(&self, request_token: &str) -> String {
        format!("{}/oauth/authorize?key={}&token={}", 
            self.base_url(), 
            self.consumer_key,
            request_token)
    }

    /// Get access token (step 3 of OAuth flow)
    async fn get_access_token(&self, request_token: &str, request_token_secret: &str, verifier: &str) -> Result<(String, String)> {
        debug!("Getting OAuth access token");
        let url = format!("{}/oauth/access_token", self.base_url());

        let nonce: u64 = rand::thread_rng().gen();
        let timestamp = Utc::now().timestamp();
        let timestamp_str = timestamp.to_string();
        let nonce_str = nonce.to_string();

        // Create OAuth parameters
        let mut params = vec![
            ("oauth_consumer_key", self.consumer_key.as_str()),
            ("oauth_token", request_token),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", &timestamp_str),
            ("oauth_nonce", &nonce_str),
            ("oauth_version", "1.0"),
            ("oauth_verifier", verifier),
        ];

        // Sort parameters
        params.sort_by(|a, b| a.0.cmp(&b.0));

        // Create parameter string
        let param_str = params.iter()
            .map(|(k, v)| format!("{}={}", 
                percent_encode(k.as_bytes(), NON_ALPHANUMERIC),
                percent_encode(v.as_bytes(), NON_ALPHANUMERIC)))
            .collect::<Vec<String>>()
            .join("&");

        // Create signature base string
        let base = format!("{}&{}&{}", 
            "GET", 
            percent_encode(url.as_bytes(), NON_ALPHANUMERIC),
            percent_encode(param_str.as_bytes(), NON_ALPHANUMERIC));

        // Create signing key
        let key = format!("{}&{}", 
            percent_encode(self.consumer_secret.as_bytes(), NON_ALPHANUMERIC),
            percent_encode(request_token_secret.as_bytes(), NON_ALPHANUMERIC));

        // Generate signature
        let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes())
            .map_err(|e| OptionsError::Other(e.to_string()))?;
        mac.update(base.as_bytes());
        let result = mac.finalize().into_bytes();
        let signature = BASE64.encode(result);

        // Add signature to parameters
        params.push(("oauth_signature", &signature));

        // Create Authorization header
        let auth_header = params.iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, percent_encode(v.as_bytes(), NON_ALPHANUMERIC)))
            .collect::<Vec<String>>()
            .join(", ");

        // Send request
        let response = self.http_client.get(&url)
            .header("Authorization", format!("OAuth {}", auth_header))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to get access token: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_else(|_| "No response body".to_string());
            return Err(OptionsError::Other(format!("Failed to get access token: HTTP {} - {}", status, text)));
        }

        // Parse response
        let body = response.text().await
            .map_err(|e| OptionsError::ParseError(format!("Failed to read response: {}", e)))?;

        // Parse oauth_token and oauth_token_secret from response
        let params: Vec<(String, String)> = body.split('&')
            .filter_map(|pair| {
                let mut parts = pair.split('=');
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    Some((key.to_string(), value.to_string()))
                } else {
                    None
                }
            })
            .collect();

        let token = params.iter()
            .find(|(k, _)| k == "oauth_token")
            .map(|(_, v)| v.clone())
            .ok_or_else(|| OptionsError::ParseError("oauth_token not found in response".to_string()))?;

        let token_secret = params.iter()
            .find(|(k, _)| k == "oauth_token_secret")
            .map(|(_, v)| v.clone())
            .ok_or_else(|| OptionsError::ParseError("oauth_token_secret not found in response".to_string()))?;

        debug!("Got access token: {}", token);

        // Store the token in a separate scope to ensure the MutexGuard is dropped
        {
            let mut token_guard = self.token.lock().unwrap();
            *token_guard = Some(OAuthToken::new(token.clone(), token_secret.clone()));
        } // token_guard is dropped here

        Ok((token, token_secret))
    }

    /// Renew access token
    async fn renew_access_token(&self) -> Result<()> {
        debug!("Renewing OAuth access token");
        let url = format!("{}/oauth/renew_access_token", self.base_url());

        // Extract token data without holding the lock across await points
        let token_str: String;
        let token_secret: String;
        {
            let token_guard = self.token.lock().unwrap();
            let token = token_guard.as_ref()
                .ok_or_else(|| OptionsError::Other("No access token available to renew".to_string()))?;
            token_str = token.token.clone();
            token_secret = token.secret.clone();
        } // token_guard is dropped here

        let nonce: u64 = rand::thread_rng().gen();
        let timestamp = Utc::now().timestamp();
        let timestamp_str = timestamp.to_string();
        let nonce_str = nonce.to_string();

        // Create OAuth parameters
        let mut params = vec![
            ("oauth_consumer_key", self.consumer_key.as_str()),
            ("oauth_token", &token_str),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", &timestamp_str),
            ("oauth_nonce", &nonce_str),
            ("oauth_version", "1.0"),
        ];

        // Sort parameters
        params.sort_by(|a, b| a.0.cmp(&b.0));

        // Create parameter string
        let param_str = params.iter()
            .map(|(k, v)| format!("{}={}", 
                percent_encode(k.as_bytes(), NON_ALPHANUMERIC),
                percent_encode(v.as_bytes(), NON_ALPHANUMERIC)))
            .collect::<Vec<String>>()
            .join("&");

        // Create signature base string
        let base = format!("{}&{}&{}", 
            "GET", 
            percent_encode(url.as_bytes(), NON_ALPHANUMERIC),
            percent_encode(param_str.as_bytes(), NON_ALPHANUMERIC));

        // Create signing key
        let key = format!("{}&{}", 
            percent_encode(self.consumer_secret.as_bytes(), NON_ALPHANUMERIC),
            percent_encode(token_secret.as_bytes(), NON_ALPHANUMERIC));

        // Generate signature
        let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes())
            .map_err(|e| OptionsError::Other(e.to_string()))?;
        mac.update(base.as_bytes());
        let result = mac.finalize().into_bytes();
        let signature = BASE64.encode(result);

        // Add signature to parameters
        params.push(("oauth_signature", &signature));

        // Create Authorization header
        let auth_header = params.iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, percent_encode(v.as_bytes(), NON_ALPHANUMERIC)))
            .collect::<Vec<String>>()
            .join(", ");

        // Send request
        let response = self.http_client.get(&url)
            .header("Authorization", format!("OAuth {}", auth_header))
            .send()
            .await
            .map_err(|e| OptionsError::Other(format!("Failed to renew access token: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_else(|_| "No response body".to_string());
            return Err(OptionsError::Other(format!("Failed to renew access token: HTTP {} - {}", status, text)));
        }

        // Update the token's last used time in a separate scope to ensure the MutexGuard is dropped
        {
            let mut token_guard = self.token.lock().unwrap();
            if let Some(token) = token_guard.as_mut() {
                token.update_last_used();
            }
        } // token_guard is dropped here

        debug!("Successfully renewed access token");
        Ok(())
    }

    /// Check if we have a valid token
    fn has_valid_token(&self) -> bool {
        let token_guard = self.token.lock().unwrap();
        if let Some(token) = token_guard.as_ref() {
            !token.is_expired()
        } else {
            false
        }
    }

    /// Sign a request with OAuth credentials
    async fn sign_request(&self, req: RequestBuilder, method: &str, url: &str, query: &[(String, String)]) -> Result<RequestBuilder> {
        // Check if token is valid, try to renew if not
        let needs_renewal = {
            let token_guard = self.token.lock().unwrap();
            match token_guard.as_ref() {
                Some(token) => token.is_expired(),
                None => false
            }
        };

        let has_token = {
            let token_guard = self.token.lock().unwrap();
            token_guard.is_some()
        };

        if needs_renewal && has_token {
            debug!("Token expired, attempting to renew");
            if let Err(e) = self.renew_access_token().await {
                warn!("Failed to renew token: {}", e);
                // Token renewal failed, we need a new token
                return Err(OptionsError::Other("Access token expired and renewal failed. Please re-authorize.".to_string()));
            }
        } else if !has_token {
            // No token available
            return Err(OptionsError::Other("No access token available. Please authorize first.".to_string()));
        }

        // Get the token and clone the necessary data
        let token_str: String;
        let token_secret: String;
        {
            let mut token_guard = self.token.lock().unwrap();
            let token = token_guard.as_mut()
                .ok_or_else(|| OptionsError::Other("No access token available".to_string()))?;

            // Update last used time
            token.update_last_used();

            // Clone the token data we need
            token_str = token.token.clone();
            token_secret = token.secret.clone();
        } // token_guard is dropped here

        let nonce: u64 = rand::thread_rng().gen();
        let timestamp = Utc::now().timestamp();
        let timestamp_str = timestamp.to_string();
        let nonce_str = nonce.to_string();

        let mut params: Vec<(String, String)> = Vec::new();
        params.push(("oauth_consumer_key".into(), self.consumer_key.clone()));
        params.push(("oauth_token".into(), token_str.clone()));
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
        let key = format!("{}&{}", percent_encode(self.consumer_secret.as_bytes(), NON_ALPHANUMERIC), percent_encode(token_secret.as_bytes(), NON_ALPHANUMERIC));
        let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes()).map_err(|e| OptionsError::Other(e.to_string()))?;
        mac.update(base.as_bytes());
        let result = mac.finalize().into_bytes();
        let signature = BASE64.encode(result);

        let header_params = vec![
            ("oauth_consumer_key", self.consumer_key.as_str()),
            ("oauth_token", &token_str),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", &timestamp_str),
            ("oauth_nonce", &nonce_str),
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
        let http = reqwest::Client::new();
        Self {
            http: http.clone(),
            creds: OAuthCreds::new(&cfg, http),
            sandbox: cfg.sandbox,
        }
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str, query: &[(String, String)]) -> Result<T> {
        let base = if self.sandbox { "https://apisb.etrade.com" } else { "https://api.etrade.com" };
        let url = format!("{}{}", base, path);
        let req = self.http.get(&url);
        let signed = self.creds.sign_request(req, "GET", &url, query).await?;
        let mut req_with_query = signed;
        for (k, v) in query {
            req_with_query = req_with_query.query(&[(k, v)]);
        }

        let res = req_with_query.send().await.map_err(|e| OptionsError::Other(e.to_string()))?;

        // Check for 401 Unauthorized and attempt to renew token
        if res.status() == StatusCode::UNAUTHORIZED {
            debug!("Received 401 Unauthorized, attempting to renew token");
            // Try to renew the token
            if let Err(e) = self.creds.renew_access_token().await {
                warn!("Failed to renew token: {}", e);
                return Err(OptionsError::Other("Access token expired and renewal failed. Please re-authorize.".to_string()));
            }

            // Retry the request with the renewed token
            debug!("Token renewed, retrying request");
            let req = self.http.get(&url);
            let signed = self.creds.sign_request(req, "GET", &url, query).await?;
            let mut req_with_query = signed;
            for (k, v) in query {
                req_with_query = req_with_query.query(&[(k, v)]);
            }

            let res = req_with_query.send().await.map_err(|e| OptionsError::Other(e.to_string()))?;
            let res = res.error_for_status().map_err(|e| OptionsError::Other(e.to_string()))?;
            return Ok(res.json::<T>().await.map_err(|e| OptionsError::ParseError(e.to_string()))?);
        }

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
            let naive_datetime = date.and_hms_opt(16,0,0).unwrap();
            let contract = OptionContract::new(symbol.to_string(), option_type, pair.strike_price, Utc.from_utc_datetime(&naive_datetime));
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

    /// Get a request token (step 1 of OAuth flow)
    pub async fn get_request_token(&self) -> Result<(String, String)> {
        self.creds.get_request_token().await
    }

    /// Get the authorization URL (step 2 of OAuth flow)
    pub fn get_authorize_url(&self, request_token: &str) -> String {
        self.creds.get_authorize_url(request_token)
    }

    /// Get an access token (step 3 of OAuth flow)
    pub async fn get_access_token(&self, request_token: &str, request_token_secret: &str, verifier: &str) -> Result<(String, String)> {
        self.creds.get_access_token(request_token, request_token_secret, verifier).await
    }

    /// Renew the access token
    pub async fn renew_access_token(&self) -> Result<()> {
        self.creds.renew_access_token().await
    }

    /// Check if the client has a valid token
    pub fn has_valid_token(&self) -> bool {
        self.creds.has_valid_token()
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
    #[serde(rename = "lastTrade")]
    pub last_trade: Option<f64>,
    #[serde(rename = "totalVolume")]
    pub total_volume: Option<u64>,
}
