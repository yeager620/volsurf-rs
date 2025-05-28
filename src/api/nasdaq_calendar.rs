use crate::error::{OptionsError, Result};
use chrono::NaiveDate;
use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, REFERER, USER_AGENT};
use serde::Deserialize;

static NASDAQ_HEADERS: Lazy<HeaderMap> = Lazy::new(|| {
    let mut h = HeaderMap::new();
    h.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        ),
    );
    h.insert(ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
    h.insert(REFERER, HeaderValue::from_static("https://www.nasdaq.com/"));
    h.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    h.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    h.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    h.insert("accept-language", HeaderValue::from_static("en-US,en;q=0.9"));
    h
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClass {
    Earnings,
    Dividend,
    Split,
}

#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub symbol: String,
    pub date: NaiveDate,
    pub description: String,
    pub class_: EventClass,
}

#[derive(Deserialize)]
struct GenericNasdaqRow {
    symbol: String,
    #[serde(flatten)]
    extra: serde_json::Value,
}

async fn fetch_rows(
    client: &reqwest::Client,
    url: &str,
    date: NaiveDate,
) -> Result<Vec<GenericNasdaqRow>> {
    #[derive(Deserialize)]
    struct Wrapper {
        data: serde_json::Value,
    }

    let wrapper: Wrapper = client
        .get(url)
        .headers(NASDAQ_HEADERS.clone())
        .query(&[("date", date.format("%Y-%m-%d").to_string())])
        .send()
        .await
        .map_err(|e| OptionsError::Other(format!("Request failed: {}", e)))?
        .json()
        .await
        .map_err(|e| OptionsError::Other(format!("Failed to parse response: {}", e)))?;

    let rows_path = wrapper
        .data
        .pointer("/rows")
        .or_else(|| wrapper.data.pointer("/calendar/rows"))
        .ok_or_else(|| OptionsError::Other("Unexpected JSON shape".to_string()))?;

    let rows: Vec<GenericNasdaqRow> = serde_json::from_value(rows_path.clone())
        .map_err(|e| OptionsError::Other(format!("Failed to parse rows: {}", e)))?;
    Ok(rows)
}

async fn get_events(
    endpoint: &str,
    date: NaiveDate,
    class_: EventClass,
    date_field: &str,
) -> Result<Vec<CalendarEvent>> {
    let client = reqwest::Client::new();
    let rows = fetch_rows(&client, endpoint, date).await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let d = r.extra.get(date_field)?.as_str()?;
            let date = NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
            Some(CalendarEvent {
                symbol: r.symbol,
                date,
                description: match class_ {
                    EventClass::Earnings => "Earnings".to_string(),
                    EventClass::Dividend => "Dividend".to_string(),
                    EventClass::Split => "Split".to_string(),
                },
                class_,
            })
        })
        .collect())
}

pub async fn earnings_on(date: NaiveDate) -> Result<Vec<CalendarEvent>> {
    get_events(
        "https://api.nasdaq.com/api/calendar/earnings",
        date,
        EventClass::Earnings,
        "reportDate",
    )
    .await
}

pub async fn dividends_on(date: NaiveDate) -> Result<Vec<CalendarEvent>> {
    get_events(
        "https://api.nasdaq.com/api/calendar/dividends",
        date,
        EventClass::Dividend,
        "exOrEffDate",
    )
    .await
}

pub async fn splits_on(date: NaiveDate) -> Result<Vec<CalendarEvent>> {
    get_events(
        "https://api.nasdaq.com/api/calendar/splits",
        date,
        EventClass::Split,
        "splitDate",
    )
    .await
}
