//! The `tfs` URL parameter: a base64 protobuf describing the search, as built by
//! the Google Flights front end. Field numbers follow the fast-flights schema; a
//! repeated airport field lets one search cover several airports per side.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use domain::ProviderQuery;

const DATA: u32 = 3;
const PASSENGERS: u32 = 8;
const SEAT: u32 = 9;
const BAGGAGE: u32 = 13;
const TRIP: u32 = 19;
const LEG_DATE: u32 = 2;
const LEG_MAX_STOPS: u32 = 5;
const LEG_FROM: u32 = 13;
const LEG_TO: u32 = 14;
const AIRPORT_CODE: u32 = 2;
const CHECKED_BAGS: u32 = 3;

const ECONOMY: u64 = 1;
const ADULT: u8 = 1;
const ROUND_TRIP: u64 = 1;
const ONE_WAY: u64 = 2;

fn varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn tag(field: u32, wire: u8, out: &mut Vec<u8>) {
    varint(u64::from(field) << 3 | u64::from(wire), out);
}

fn number(field: u32, value: u64, out: &mut Vec<u8>) {
    tag(field, 0, out);
    varint(value, out);
}

fn bytes(field: u32, payload: &[u8], out: &mut Vec<u8>) {
    tag(field, 2, out);
    varint(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

fn airport(code: &str) -> Vec<u8> {
    let mut out = Vec::new();
    bytes(AIRPORT_CODE, code.as_bytes(), &mut out);
    out
}

pub fn encode(query: &ProviderQuery) -> String {
    let mut info = Vec::new();
    bytes(
        PASSENGERS,
        &vec![ADULT; usize::from(query.passengers)],
        &mut info,
    );
    number(SEAT, ECONOMY, &mut info);
    if query.checked_bags > 0 {
        let mut baggage = Vec::new();
        number(CHECKED_BAGS, u64::from(query.checked_bags), &mut baggage);
        bytes(BAGGAGE, &baggage, &mut info);
    }
    let trip = if query.is_round_trip() {
        ROUND_TRIP
    } else {
        ONE_WAY
    };
    number(TRIP, trip, &mut info);
    for leg in &query.legs {
        let mut data = Vec::new();
        bytes(LEG_DATE, leg.date_from.to_string().as_bytes(), &mut data);
        number(LEG_MAX_STOPS, u64::from(query.max_stops), &mut data);
        for code in &leg.from {
            bytes(LEG_FROM, &airport(code.as_str()), &mut data);
        }
        for code in &leg.to {
            bytes(LEG_TO, &airport(code.as_str()), &mut data);
        }
        bytes(DATA, &data, &mut info);
    }
    STANDARD.encode(info)
}

#[cfg(test)]
mod tests {
    use domain::{Iata, QueryLeg};

    use super::*;

    fn query(legs: Vec<QueryLeg>) -> ProviderQuery {
        ProviderQuery {
            legs,
            max_stops: 1,
            passengers: 1,
            currency: "EUR".into(),
            checked_bags: 0,
        }
    }

    fn leg(from: &str, to: &str, date: &str) -> QueryLeg {
        QueryLeg::on(
            vec![Iata::new(from).unwrap()],
            vec![Iata::new(to).unwrap()],
            date.parse().unwrap(),
        )
    }

    #[test]
    fn matches_the_query_recorded_from_the_site() {
        let round_trip = query(vec![
            leg("FCO", "NRT", "2026-12-10"),
            leg("NRT", "FCO", "2026-12-20"),
        ]);
        assert_eq!(
            encode(&round_trip),
            "QgEBSAGYAQEaHBIKMjAyNi0xMi0xMCgBagUSA0ZDT3IFEgNOUlQaHBIKMjAyNi0xMi0yMCgBagUSA05SVHIFEgNGQ08="
        );
    }

    #[test]
    fn encodes_every_airport_passenger_and_bag() {
        let mut q = query(vec![QueryLeg::on(
            ["FCO", "MXP"]
                .iter()
                .map(|c| Iata::new(c).unwrap())
                .collect(),
            ["NRT", "HND"]
                .iter()
                .map(|c| Iata::new(c).unwrap())
                .collect(),
            "2026-12-10".parse().unwrap(),
        )]);
        q.passengers = 2;
        q.checked_bags = 1;
        let raw = STANDARD.decode(encode(&q)).unwrap();
        for code in ["FCO", "MXP", "NRT", "HND"] {
            assert_eq!(
                raw.windows(3).filter(|w| *w == code.as_bytes()).count(),
                1,
                "{code}"
            );
        }
        assert!(raw.starts_with(&[0x42, 0x02, 0x01, 0x01]), "two adults");
        assert!(
            raw.windows(4).any(|w| w == [0x6a, 0x02, 0x18, 0x01]),
            "one checked bag"
        );
    }

    #[test]
    fn varints_span_bytes() {
        let mut out = Vec::new();
        varint(300, &mut out);
        assert_eq!(out, [0xac, 0x02]);
    }
}
