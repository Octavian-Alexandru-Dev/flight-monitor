use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use domain::{Capabilities, Decimal, Iata, Itinerary, ProviderId, ProviderQuery, Segment, Url};
use serde::Deserialize;

use crate::{FlightProvider, ProviderError};

const ID: &str = "travelpayouts";
const ENDPOINT: &str = "https://api.travelpayouts.com/aviasales/v3/prices_for_dates";
const BOOKING_BASE: &str = "https://www.aviasales.com";
const LIMIT: &str = "1000";

/// Aviasales Data API: cached prices (hours to days old), cheap to query.
/// Used only to scan which dates and destinations deserve a live check.
pub struct Travelpayouts {
    client: reqwest::Client,
    token: String,
    endpoint: String,
    capabilities: Capabilities,
}

impl Travelpayouts {
    pub fn new(token: &str) -> Result<Self, ProviderError> {
        Self::with_endpoint(token, ENDPOINT)
    }

    pub fn with_endpoint(token: &str, endpoint: &str) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .gzip(true)
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        Ok(Self {
            client,
            token: token.to_string(),
            endpoint: endpoint.to_string(),
            capabilities: Capabilities {
                multi_airport_query: false,
                max_airports_per_side: 1,
                city_query: true,
                max_stops_filter: false,
                round_trip: false,
                date_range: true,
                open_destination: true,
                live: false,
                min_interval: Duration::from_secs(1),
            },
        })
    }
}

#[derive(Deserialize)]
struct Response {
    success: bool,
    #[serde(default)]
    data: Option<Vec<Quote>>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Deserialize)]
struct Quote {
    origin: String,
    destination: String,
    #[serde(default)]
    origin_airport: Option<String>,
    #[serde(default)]
    destination_airport: Option<String>,
    price: serde_json::Number,
    #[serde(default)]
    airline: String,
    #[serde(default)]
    flight_number: serde_json::Value,
    departure_at: DateTime<FixedOffset>,
    #[serde(default)]
    transfers: u8,
    #[serde(default)]
    duration_to: Option<u32>,
    #[serde(default)]
    duration: Option<u32>,
    #[serde(default)]
    link: Option<String>,
}

impl Quote {
    fn into_itinerary(self, currency: &str, fetched_at: DateTime<Utc>) -> Option<Itinerary> {
        let from = Iata::new(self.origin_airport.as_deref().unwrap_or(&self.origin)).ok()?;
        let to = Iata::new(
            self.destination_airport
                .as_deref()
                .unwrap_or(&self.destination),
        )
        .ok()?;
        let minutes = self.duration_to.or(self.duration).unwrap_or(0);
        let departure = self.departure_at.naive_local();
        let price: Decimal = self.price.to_string().parse().ok()?;
        let flight_number = match self.flight_number {
            serde_json::Value::String(s) => s,
            serde_json::Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        let mut itinerary = Itinerary::new(
            // The cache only knows the end points: one synthetic segment per quote.
            vec![Segment {
                from,
                to,
                departure,
                arrival: departure + chrono::Duration::minutes(i64::from(minutes)),
                duration_minutes: minutes,
                airline_name: self.airline.clone(),
                airline: self.airline,
                flight_number,
            }],
            price,
            currency.to_string(),
            ProviderId::new(ID),
            fetched_at,
        )
        .ok()?;
        itinerary.booking_url = self
            .link
            .and_then(|link| Url::parse(&format!("{BOOKING_BASE}{link}")).ok());
        Some(itinerary)
    }
}

/// Parses an answer, keeping quotes inside the query's dates and stop limit.
pub fn parse_response(
    body: &[u8],
    query: &ProviderQuery,
    fetched_at: DateTime<Utc>,
) -> Result<Vec<Itinerary>, ProviderError> {
    let response: Response = serde_json::from_slice(body)
        .map_err(|e| ProviderError::Parse(format!("invalid Travelpayouts JSON: {e}")))?;
    if !response.success {
        return Err(ProviderError::Parse(
            response
                .error
                .unwrap_or_else(|| "request not successful".into()),
        ));
    }
    let currency = response
        .currency
        .unwrap_or_else(|| query.currency.clone())
        .to_ascii_uppercase();
    let Some(leg) = query.legs.first() else {
        return Err(ProviderError::Unsupported);
    };
    Ok(response
        .data
        .unwrap_or_default()
        .into_iter()
        .filter(|q| q.transfers <= query.max_stops)
        .filter_map(|q| q.into_itinerary(&currency, fetched_at))
        .filter(|i| {
            let day = i.departure_date();
            day >= leg.date_from && day <= leg.date_to
        })
        .collect())
}

#[async_trait]
impl FlightProvider for Travelpayouts {
    fn id(&self) -> ProviderId {
        ProviderId::new(ID)
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn search(&self, query: &ProviderQuery) -> Result<Vec<Itinerary>, ProviderError> {
        let [leg] = query.legs.as_slice() else {
            return Err(ProviderError::Unsupported);
        };
        let ([origin], destination) = (leg.from.as_slice(), leg.to.as_slice()) else {
            return Err(ProviderError::Unsupported);
        };
        if destination.len() > 1
            || leg.date_from.format("%Y-%m").to_string() != leg.date_to.format("%Y-%m").to_string()
        {
            return Err(ProviderError::Unsupported);
        }
        let departure = if leg.date_from == leg.date_to {
            leg.date_from.format("%Y-%m-%d").to_string()
        } else {
            leg.date_from.format("%Y-%m").to_string()
        };
        let mut params = vec![
            ("origin", origin.to_string()),
            ("departure_at", departure),
            ("one_way", "true".into()),
            ("direct", (query.max_stops == 0).to_string()),
            ("currency", query.currency.to_ascii_lowercase()),
            ("sorting", "price".into()),
            ("unique", "false".into()),
            ("limit", LIMIT.into()),
            ("page", "1".into()),
        ];
        if let Some(to) = destination.first() {
            params.push(("destination", to.to_string()));
        }
        let response = self
            .client
            .get(&self.endpoint)
            .header("X-Access-Token", &self.token)
            .query(&params)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        match response.status().as_u16() {
            200 => {}
            429 => return Err(ProviderError::RateLimited),
            401 | 403 => return Err(ProviderError::Blocked),
            status => return Err(ProviderError::Network(format!("HTTP {status}"))),
        }
        let body = response
            .bytes()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        parse_response(&body, query, Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use domain::QueryLeg;

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../fixtures/travelpayouts/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn query(to: &[&str], max_stops: u8) -> ProviderQuery {
        ProviderQuery {
            legs: vec![QueryLeg::between(
                vec![Iata::new("FCO").unwrap()],
                to.iter().map(|c| Iata::new(c).unwrap()).collect(),
                date("2026-12-01"),
                date("2026-12-20"),
            )],
            max_stops,
            passengers: 1,
            currency: "EUR".into(),
            checked_bags: 0,
        }
    }

    #[test]
    fn parses_month_quotes_within_the_window() {
        let quotes = parse_response(
            &fixture("prices_for_dates_route"),
            &query(&["NRT"], 1),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(
            quotes.len(),
            4,
            "one outside the window, one with too many stops"
        );
        let cheapest = quotes.iter().min_by_key(|q| q.price).unwrap();
        assert_eq!(cheapest.price, Decimal::from(472));
        assert_eq!(cheapest.origin().as_str(), "FCO");
        assert_eq!(cheapest.destination().as_str(), "NRT");
        assert_eq!(cheapest.departure_date(), date("2026-12-09"));
        assert_eq!(cheapest.currency, "EUR");
        assert!(
            cheapest
                .booking_url
                .as_ref()
                .unwrap()
                .as_str()
                .starts_with("https://www.aviasales.com/search/")
        );
    }

    #[test]
    fn parses_open_destination_quotes() {
        let quotes = parse_response(
            &fixture("prices_for_dates_anywhere"),
            &query(&[], 2),
            Utc::now(),
        )
        .unwrap();
        let destinations: Vec<&str> = quotes.iter().map(|q| q.destination().as_str()).collect();
        assert_eq!(destinations, ["BCN", "ATH", "JFK"]);
    }

    #[test]
    fn reports_api_errors() {
        let error = br#"{"success": false, "data": null, "error": "invalid token"}"#;
        assert_eq!(
            parse_response(error, &query(&["NRT"], 1), Utc::now()),
            Err(ProviderError::Parse("invalid token".into()))
        );
        assert!(matches!(
            parse_response(b"<html>", &query(&["NRT"], 1), Utc::now()),
            Err(ProviderError::Parse(_))
        ));
    }

    #[tokio::test]
    async fn rejects_queries_it_cannot_express() {
        let provider = Travelpayouts::new("token").unwrap();
        let mut two_months = query(&["NRT"], 1);
        two_months.legs[0].date_to = date("2027-01-10");
        assert_eq!(
            provider.search(&two_months).await,
            Err(ProviderError::Unsupported)
        );
        let mut many = query(&["NRT", "HND"], 1);
        many.legs[0].date_to = date("2026-12-02");
        assert_eq!(
            provider.search(&many).await,
            Err(ProviderError::Unsupported)
        );
    }
}
