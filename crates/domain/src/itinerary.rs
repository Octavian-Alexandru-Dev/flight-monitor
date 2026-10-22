use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{DomainError, Iata, ProviderId};

const OVERNIGHT_MIN_MINUTES: i64 = 6 * 60;

/// One flight. Times are local to the departure/arrival airport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub from: Iata,
    pub to: Iata,
    pub departure: NaiveDateTime,
    pub arrival: NaiveDateTime,
    pub duration_minutes: u32,
    pub airline: String,
    pub airline_name: String,
    pub flight_number: String,
}

impl Segment {
    pub fn code(&self) -> String {
        format!("{}{}", self.airline, self.flight_number)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Layover {
    pub arrival_airport: Iata,
    pub departure_airport: Iata,
    pub minutes: i64,
    pub overnight: bool,
}

impl Layover {
    fn between(arriving: &Segment, departing: &Segment) -> Self {
        let minutes = (departing.departure - arriving.arrival).num_minutes();
        Self {
            arrival_airport: arriving.to.clone(),
            departure_airport: departing.from.clone(),
            minutes,
            overnight: minutes >= OVERNIGHT_MIN_MINUTES
                && spans_night(arriving.arrival, departing.departure),
        }
    }

    pub fn changes_airport(&self) -> bool {
        self.arrival_airport != self.departure_airport
    }
}

fn spans_night(from: NaiveDateTime, to: NaiveDateTime) -> bool {
    let three_am = NaiveTime::from_hms_opt(3, 0, 0).unwrap_or_default();
    let candidate = from.date().and_time(three_am);
    let next = candidate + chrono::Duration::days(1);
    (candidate >= from && candidate <= to) || (next >= from && next <= to)
}

/// A bookable ticket: one or more segments sold together at one price.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ItineraryData")]
pub struct Itinerary {
    segments: Vec<Segment>,
    pub price: Decimal,
    pub currency: String,
    pub booking_url: Option<Url>,
    pub source: ProviderId,
    pub fetched_at: DateTime<Utc>,
    /// Set when the price covers a round trip returning on this date.
    pub return_date: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct ItineraryData {
    segments: Vec<Segment>,
    price: Decimal,
    currency: String,
    booking_url: Option<Url>,
    source: ProviderId,
    fetched_at: DateTime<Utc>,
    return_date: Option<NaiveDate>,
}

impl TryFrom<ItineraryData> for Itinerary {
    type Error = DomainError;

    fn try_from(data: ItineraryData) -> Result<Self, Self::Error> {
        let mut itinerary = Self::new(
            data.segments,
            data.price,
            data.currency,
            data.source,
            data.fetched_at,
        )?;
        itinerary.booking_url = data.booking_url;
        itinerary.return_date = data.return_date;
        Ok(itinerary)
    }
}

impl Itinerary {
    pub fn new(
        segments: Vec<Segment>,
        price: Decimal,
        currency: String,
        source: ProviderId,
        fetched_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if segments.is_empty() {
            return Err(DomainError::InvalidSearch(
                "an itinerary needs at least one segment".into(),
            ));
        }
        Ok(Self {
            segments,
            price,
            currency,
            booking_url: None,
            source,
            fetched_at,
            return_date: None,
        })
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    fn first(&self) -> &Segment {
        &self.segments[0]
    }

    fn last(&self) -> &Segment {
        &self.segments[self.segments.len() - 1]
    }

    pub fn origin(&self) -> &Iata {
        &self.first().from
    }

    pub fn destination(&self) -> &Iata {
        &self.last().to
    }

    pub fn departure(&self) -> NaiveDateTime {
        self.first().departure
    }

    pub fn arrival(&self) -> NaiveDateTime {
        self.last().arrival
    }

    pub fn departure_date(&self) -> NaiveDate {
        self.departure().date()
    }

    pub fn stops(&self) -> usize {
        self.segments.len() - 1
    }

    pub fn layovers(&self) -> Vec<Layover> {
        self.segments
            .windows(2)
            .map(|pair| Layover::between(&pair[0], &pair[1]))
            .collect()
    }

    pub fn total_minutes(&self) -> i64 {
        let flying: i64 = self
            .segments
            .iter()
            .map(|s| i64::from(s.duration_minutes))
            .sum();
        flying + self.layovers().iter().map(|l| l.minutes).sum::<i64>()
    }

    pub fn airlines(&self) -> Vec<&str> {
        let mut codes: Vec<&str> = self.segments.iter().map(|s| s.airline.as_str()).collect();
        codes.dedup();
        codes
    }

    /// Natural key: the flights flown plus, for round-trip fares, the return date.
    pub fn key(&self) -> String {
        let flights: Vec<String> = self
            .segments
            .iter()
            .map(|s| format!("{}@{}", s.code(), s.departure.format("%Y-%m-%dT%H:%M")))
            .collect();
        match self.return_date {
            Some(date) => format!("{}|R{date}", flights.join("+")),
            None => flights.join("+"),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn segment(from: &str, to: &str, dep: &str, arr: &str, minutes: u32) -> Segment {
        Segment {
            from: Iata::new(from).unwrap(),
            to: Iata::new(to).unwrap(),
            departure: dep.parse().unwrap(),
            arrival: arr.parse().unwrap(),
            duration_minutes: minutes,
            airline: "EY".into(),
            airline_name: "Etihad".into(),
            flight_number: "84".into(),
        }
    }

    fn itinerary() -> Itinerary {
        Itinerary::new(
            vec![
                segment(
                    "FCO",
                    "AUH",
                    "2026-12-10T22:00:00",
                    "2026-12-11T07:05:00",
                    365,
                ),
                segment(
                    "AUH",
                    "NRT",
                    "2026-12-11T21:05:00",
                    "2026-12-12T11:40:00",
                    575,
                ),
            ],
            Decimal::new(490, 0),
            "EUR".into(),
            ProviderId::new("test"),
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn computes_layover_and_total_duration() {
        let it = itinerary();
        let layovers = it.layovers();
        assert_eq!(layovers.len(), 1);
        assert_eq!(layovers[0].minutes, 840);
        assert!(!layovers[0].overnight);
        assert_eq!(it.total_minutes(), 1780);
        assert_eq!(it.stops(), 1);
    }

    #[test]
    fn detects_overnight_layover() {
        let it = Itinerary::new(
            vec![
                segment(
                    "FCO",
                    "IST",
                    "2026-12-10T19:45:00",
                    "2026-12-11T00:20:00",
                    155,
                ),
                segment(
                    "IST",
                    "NRT",
                    "2026-12-11T15:25:00",
                    "2026-12-12T08:40:00",
                    675,
                ),
            ],
            Decimal::new(584, 0),
            "EUR".into(),
            ProviderId::new("test"),
            Utc::now(),
        )
        .unwrap();
        assert!(it.layovers()[0].overnight);
    }

    #[test]
    fn rejects_empty_segments() {
        let empty: Result<Itinerary, _> = serde_json::from_value(serde_json::json!({
            "segments": [], "price": "1", "currency": "EUR", "booking_url": null,
            "source": "x", "fetched_at": "2026-01-01T00:00:00Z", "return_date": null
        }));
        assert!(empty.is_err());
    }

    #[test]
    fn key_identifies_flights_and_return_date() {
        let mut it = itinerary();
        assert_eq!(it.key(), "EY84@2026-12-10T22:00+EY84@2026-12-11T21:05");
        it.return_date = "2026-12-20".parse().ok();
        assert!(it.key().ends_with("|R2026-12-20"));
    }
}
