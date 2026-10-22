mod browser;
mod page;
mod parse;
mod tfs;

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use domain::{Capabilities, Itinerary, ProviderId, ProviderQuery, Url};

pub use browser::{ChromeSource, HeadlessBrowser};
pub use parse::GoogleReply;

use crate::{FlightProvider, ProviderError};

const ID: &str = "google_flights";
const SEARCH_URL: &str = "https://www.google.com/travel/flights/search";
const MAX_AIRPORTS_PER_SIDE: usize = 7;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
/// Pre-accepted consent, so EU requests are not redirected to consent.google.com.
const CONSENT_COOKIE: &str = "SOCS";
const CONSENT_VALUE: &str = "CAESHAgBEhJnd3NfMjAyMzA4MTAtMF9SQzIaAmRlIAEaBgiAo_CmBg";

/// Live Google Flights results. A plain request first; headless Chromium when the
/// page does not embed the results.
pub struct GoogleFlights {
    http: reqwest::Client,
    browser: HeadlessBrowser,
    capabilities: Capabilities,
}

impl GoogleFlights {
    pub fn new(browser: HeadlessBrowser, min_interval: Duration) -> Result<Self, ProviderError> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(40))
            .gzip(true)
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        Ok(Self {
            http,
            browser,
            capabilities: Capabilities {
                multi_airport_query: true,
                max_airports_per_side: MAX_AIRPORTS_PER_SIDE,
                city_query: false,
                max_stops_filter: true,
                round_trip: true,
                date_range: false,
                open_destination: false,
                live: true,
                min_interval,
            },
        })
    }

    pub fn browser(&self) -> &HeadlessBrowser {
        &self.browser
    }

    async fn page(&self, url: &Url) -> Result<String, ProviderError> {
        let response = self
            .http
            .get(url.clone())
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Cookie", format!("{CONSENT_COOKIE}={CONSENT_VALUE}"))
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        let redirected = response.url().path().starts_with("/sorry");
        match status {
            _ if redirected => return Err(ProviderError::Blocked),
            200 => {}
            429 => return Err(ProviderError::RateLimited),
            403 | 503 => return Err(ProviderError::Blocked),
            other => return Err(ProviderError::Network(format!("HTTP {other}"))),
        }
        response
            .text()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))
    }
}

fn supported(query: &ProviderQuery) -> bool {
    !query.legs.is_empty()
        && query.legs.len() <= 2
        && query.legs.iter().all(|leg| {
            leg.date_from == leg.date_to
                && !leg.from.is_empty()
                && !leg.to.is_empty()
                && leg.from.len() <= MAX_AIRPORTS_PER_SIDE
                && leg.to.len() <= MAX_AIRPORTS_PER_SIDE
        })
}

pub fn search_url(query: &ProviderQuery) -> Result<Url, ProviderError> {
    Url::parse_with_params(
        SEARCH_URL,
        [
            ("tfs", tfs::encode(query).as_str()),
            ("hl", "en"),
            ("curr", query.currency.as_str()),
        ],
    )
    .map_err(|e| ProviderError::Parse(e.to_string()))
}

#[async_trait]
impl FlightProvider for GoogleFlights {
    fn id(&self) -> ProviderId {
        ProviderId::new(ID)
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn search(&self, query: &ProviderQuery) -> Result<Vec<Itinerary>, ProviderError> {
        if !supported(query) {
            return Err(ProviderError::Unsupported);
        }
        let url = search_url(query)?;
        let payload = match page::embedded_payload(&self.page(&url).await?)? {
            Some(payload) if page::flight_count(&payload) == 0 => {
                tracing::debug!(
                    query = query.describe(),
                    "results not embedded, opening the browser"
                );
                Some(self.browser.results(url.as_str()).await?)
            }
            other => other,
        };
        GoogleReply {
            url: url.to_string(),
            payload,
        }
        .itineraries(query, Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use domain::{Iata, QueryLeg};

    use super::*;

    fn leg(from: &[&str], to: &[&str]) -> QueryLeg {
        let codes = |list: &[&str]| list.iter().map(|c| Iata::new(c).unwrap()).collect();
        QueryLeg::on(codes(from), codes(to), "2026-12-10".parse().unwrap())
    }

    fn query(legs: Vec<QueryLeg>) -> ProviderQuery {
        ProviderQuery {
            legs,
            max_stops: 1,
            passengers: 1,
            currency: "EUR".into(),
            checked_bags: 0,
        }
    }

    #[test]
    fn rejects_queries_it_cannot_express() {
        let mut range = leg(&["FCO"], &["NRT"]);
        range.date_to = "2026-12-31".parse().unwrap();
        assert!(!supported(&query(vec![range])));
        assert!(!supported(&query(vec![leg(&["FCO"], &[])])));
        assert!(supported(&query(vec![leg(
            &["FCO", "CIA"],
            &["NRT", "HND"]
        )])));
    }

    #[test]
    fn builds_the_booking_url() {
        let url = search_url(&query(vec![leg(&["FCO"], &["NRT"])])).unwrap();
        assert!(
            url.as_str()
                .starts_with("https://www.google.com/travel/flights/search?tfs=")
        );
        assert!(url.as_str().ends_with("&hl=en&curr=EUR"));
    }
}
