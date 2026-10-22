use std::fmt;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::Iata;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One direction of travel. An empty `to` means "any destination".
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct QueryLeg {
    pub from: Vec<Iata>,
    pub to: Vec<Iata>,
    pub date_from: NaiveDate,
    pub date_to: NaiveDate,
}

impl QueryLeg {
    pub fn on(from: Vec<Iata>, to: Vec<Iata>, date: NaiveDate) -> Self {
        Self::between(from, to, date, date)
    }

    pub fn between(
        mut from: Vec<Iata>,
        mut to: Vec<Iata>,
        date_from: NaiveDate,
        date_to: NaiveDate,
    ) -> Self {
        from.sort();
        from.dedup();
        to.sort();
        to.dedup();
        Self {
            from,
            to,
            date_from,
            date_to,
        }
    }

    pub fn covers(&self, from: &Iata, to: &Iata, date: NaiveDate) -> bool {
        self.from.contains(from)
            && (self.to.is_empty() || self.to.contains(to))
            && date >= self.date_from
            && date <= self.date_to
    }
}

/// What the planner asks a provider. One leg is a one-way search, two legs a round trip.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProviderQuery {
    pub legs: Vec<QueryLeg>,
    pub max_stops: u8,
    pub passengers: u8,
    pub currency: String,
    pub checked_bags: u8,
}

impl ProviderQuery {
    pub fn is_round_trip(&self) -> bool {
        self.legs.len() == 2
    }

    /// Stable identity used for deduplication and caching.
    pub fn key(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn describe(&self) -> String {
        let legs: Vec<String> = self
            .legs
            .iter()
            .map(|leg| {
                let side = |codes: &[Iata]| {
                    if codes.is_empty() {
                        "*".to_string()
                    } else {
                        codes.iter().map(Iata::as_str).collect::<Vec<_>>().join(",")
                    }
                };
                let dates = if leg.date_from == leg.date_to {
                    leg.date_from.to_string()
                } else {
                    format!("{}…{}", leg.date_from, leg.date_to)
                };
                format!("{}→{} {dates}", side(&leg.from), side(&leg.to))
            })
            .collect();
        legs.join(" / ")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub multi_airport_query: bool,
    /// Airports accepted per side of a single query.
    pub max_airports_per_side: usize,
    pub city_query: bool,
    pub max_stops_filter: bool,
    pub round_trip: bool,
    /// Accepts a date range in a single query.
    pub date_range: bool,
    /// Accepts queries without a destination.
    pub open_destination: bool,
    /// `false` means cached data, good for scanning only.
    pub live: bool,
    pub min_interval: Duration,
}

/// What past runs say about a query, used to prioritise within a budget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryStats {
    pub last_run_at: Option<DateTime<Utc>>,
    pub runs: i64,
    pub runs_with_results: i64,
    pub best_price: Option<Decimal>,
}
