#![allow(clippy::unwrap_used, dead_code)]

use std::path::Path;
use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime, Utc};
use domain::{
    AlertRule, Capabilities, CostModel, DateSpec, Decimal, GroupGoal, Iata, Itinerary,
    ItineraryConstraints, LocationSet, MonitorPolicy, ProviderId, SearchDefinition, Segment,
    StayLength, TripMode,
};
use geo::GeoIndex;

pub fn geo() -> GeoIndex {
    GeoIndex::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).unwrap()
}

pub fn live_caps() -> Capabilities {
    Capabilities {
        multi_airport_query: true,
        max_airports_per_side: 7,
        city_query: false,
        max_stops_filter: true,
        round_trip: true,
        date_range: false,
        open_destination: false,
        live: true,
        min_interval: Duration::ZERO,
    }
}

pub fn scan_caps() -> Capabilities {
    Capabilities {
        multi_airport_query: false,
        max_airports_per_side: 1,
        city_query: true,
        max_stops_filter: false,
        round_trip: false,
        date_range: true,
        open_destination: true,
        live: false,
        min_interval: Duration::ZERO,
    }
}

pub fn iata(code: &str) -> Iata {
    Iata::new(code).unwrap()
}

pub fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

pub fn airport(code: &str) -> LocationSet {
    LocationSet::Airport(iata(code))
}

pub fn search(origin: LocationSet, destination: LocationSet, mode: TripMode) -> SearchDefinition {
    SearchDefinition {
        name: "test".into(),
        origin,
        destination,
        mode,
        outbound: DateSpec::Fixed {
            date: date("2026-12-10"),
        },
        stay: mode.is_round_trip().then_some(StayLength {
            min_nights: 7,
            max_nights: 9,
        }),
        return_origin: None,
        return_destination: None,
        constraints: ItineraryConstraints::default(),
        passengers: 1,
        currency: "EUR".into(),
        policy: MonitorPolicy {
            poll_interval_minutes: 360,
            query_budget: 40,
            adaptive: false,
        },
        alerts: vec![AlertRule::NewHistoricMinimum],
        via: None,
        companions: Vec::new(),
        group_goal: GroupGoal::default(),
        costs: CostModel::default(),
    }
}

pub fn window(def: &mut SearchDefinition, from: &str, to: &str) {
    def.outbound = DateSpec::Window {
        from: date(from),
        to: date(to),
    };
}

fn at(s: &str) -> NaiveDateTime {
    s.parse().unwrap()
}

/// Builds an itinerary from `(from, to, departure, arrival)` legs; times are `YYYY-MM-DDTHH:MM`.
pub fn trip(legs: &[(&str, &str, &str, &str)], price: i64, airline: &str) -> Itinerary {
    let segments = legs
        .iter()
        .enumerate()
        .map(|(i, (from, to, dep, arr))| {
            let (dep, arr) = (at(&format!("{dep}:00")), at(&format!("{arr}:00")));
            Segment {
                from: iata(from),
                to: iata(to),
                departure: dep,
                arrival: arr,
                duration_minutes: u32::try_from((arr - dep).num_minutes()).unwrap(),
                airline: airline.into(),
                airline_name: airline.into(),
                flight_number: format!("{}", 100 + i),
            }
        })
        .collect();
    Itinerary::new(
        segments,
        Decimal::from(price),
        "EUR".into(),
        ProviderId::new("test"),
        Utc::now(),
    )
    .unwrap()
}

pub fn quote(from: &str, to: &str, day: &str, price: i64) -> Itinerary {
    trip(
        &[(from, to, &format!("{day}T10:00"), &format!("{day}T12:00"))],
        price,
        "XX",
    )
}
