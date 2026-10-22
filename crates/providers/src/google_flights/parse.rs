use std::collections::HashSet;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use domain::{Decimal, Iata, Itinerary, ProviderId, ProviderQuery, Segment, Url};
use serde::Deserialize;
use serde_json::Value;

use super::ID;
use crate::ProviderError;

/// A results page: its URL and the raw payload (`None` when Google found no flights).
#[derive(Debug, Deserialize)]
pub struct GoogleReply {
    pub url: String,
    pub payload: Option<Value>,
}

impl GoogleReply {
    pub fn itineraries(
        &self,
        query: &ProviderQuery,
        fetched_at: DateTime<Utc>,
    ) -> Result<Vec<Itinerary>, ProviderError> {
        let Some(payload) = &self.payload else {
            return Ok(Vec::new());
        };
        let entries: Vec<&Value> = [&payload[2][0], &payload[3][0]]
            .into_iter()
            .filter_map(Value::as_array)
            .flatten()
            .collect();
        let booking_url = Url::parse(&self.url).ok();
        let return_date = query.legs.get(1).map(|leg| leg.date_from);

        let mut seen = HashSet::new();
        let mut itineraries = Vec::new();
        let mut malformed = 0;
        for entry in &entries {
            match parse_entry(entry, &query.currency, fetched_at) {
                Some(Some(mut itinerary)) => {
                    itinerary.booking_url = booking_url.clone();
                    itinerary.return_date = return_date;
                    if seen.insert(itinerary.key()) {
                        itineraries.push(itinerary);
                    }
                }
                Some(None) => {}
                None => malformed += 1,
            }
        }
        if malformed > 0 && itineraries.is_empty() {
            return Err(ProviderError::Parse(format!(
                "{malformed} of {} entries could not be parsed",
                entries.len()
            )));
        }
        Ok(itineraries)
    }
}

/// `None` for a malformed entry, `Some(None)` for a valid entry without a price.
fn parse_entry(
    entry: &Value,
    currency: &str,
    fetched_at: DateTime<Utc>,
) -> Option<Option<Itinerary>> {
    let segments = entry[0][2]
        .as_array()?
        .iter()
        .map(parse_segment)
        .collect::<Option<Vec<_>>>()?;
    let Some(price) = entry[1][0][1].as_i64() else {
        return Some(None);
    };
    Itinerary::new(
        segments,
        Decimal::from(price),
        currency.to_string(),
        ProviderId::new(ID),
        fetched_at,
    )
    .ok()
    .map(Some)
}

fn parse_segment(raw: &Value) -> Option<Segment> {
    let flight = &raw[22];
    Some(Segment {
        from: Iata::new(raw[3].as_str()?).ok()?,
        to: Iata::new(raw[6].as_str()?).ok()?,
        departure: local_time(&raw[20], &raw[8])?,
        arrival: local_time(&raw[21], &raw[10])?,
        duration_minutes: u32::try_from(raw[11].as_u64()?).ok()?,
        airline: flight[0].as_str()?.to_string(),
        flight_number: flight[1].as_str()?.to_string(),
        airline_name: flight[3].as_str().unwrap_or_default().to_string(),
    })
}

/// Google omits zero components: `[8]` is 08:00 and `[null, 31]` is 00:31.
fn local_time(date: &Value, time: &Value) -> Option<NaiveDateTime> {
    let part = |v: &Value, i: usize| v[i].as_u64().and_then(|n| u32::try_from(n).ok());
    let date = NaiveDate::from_ymd_opt(
        i32::try_from(date[0].as_i64()?).ok()?,
        part(date, 1)?,
        part(date, 2)?,
    )?;
    let time = NaiveTime::from_hms_opt(part(time, 0).unwrap_or(0), part(time, 1).unwrap_or(0), 0)?;
    Some(date.and_time(time))
}

#[cfg(test)]
mod tests {
    use domain::QueryLeg;

    use super::*;

    fn fixture(name: &str) -> GoogleReply {
        let path = format!(
            "{}/../../fixtures/google_flights/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn codes(list: &[&str]) -> Vec<Iata> {
        list.iter().map(|c| Iata::new(c).unwrap()).collect()
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

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn parses_multi_airport_one_way_fixture() {
        let q = query(vec![QueryLeg::on(
            codes(&["FCO", "MXP"]),
            codes(&["NRT", "HND"]),
            date("2026-12-10"),
        )]);
        let itineraries = fixture("one_way_multi_airport")
            .itineraries(&q, Utc::now())
            .unwrap();
        assert!(itineraries.len() >= 5);
        for it in &itineraries {
            assert!(
                ["FCO", "MXP"].contains(&it.origin().as_str()),
                "{}",
                it.origin()
            );
            assert!(["NRT", "HND"].contains(&it.destination().as_str()));
            assert_eq!(it.departure_date(), date("2026-12-10"));
            assert!(it.stops() <= 1);
            assert!(it.price > Decimal::ZERO);
            assert!(it.return_date.is_none());
            assert!(
                it.booking_url
                    .as_ref()
                    .is_some_and(|u| u.as_str().contains("tfs="))
            );
            for segment in it.segments() {
                assert!(!segment.flight_number.is_empty());
                assert!(segment.arrival > segment.departure - chrono::Duration::hours(24));
            }
        }
        let origins: HashSet<&str> = itineraries.iter().map(|i| i.origin().as_str()).collect();
        assert_eq!(origins.len(), 2, "both origins should appear");
    }

    #[test]
    fn layovers_are_consistent_with_reported_durations() {
        let q = query(vec![QueryLeg::on(
            codes(&["FCO", "MXP"]),
            codes(&["NRT", "HND"]),
            date("2026-12-10"),
        )]);
        for it in fixture("one_way_multi_airport")
            .itineraries(&q, Utc::now())
            .unwrap()
        {
            assert!(it.layovers().iter().all(|l| l.minutes > 0));
            assert!(it.total_minutes() > 600, "{} too short", it.key());
        }
    }

    #[test]
    fn parses_round_trip_fixture_with_return_date() {
        let q = query(vec![
            QueryLeg::on(codes(&["FCO"]), codes(&["NRT"]), date("2026-12-10")),
            QueryLeg::on(codes(&["NRT"]), codes(&["FCO"]), date("2026-12-20")),
        ]);
        let itineraries = fixture("round_trip").itineraries(&q, Utc::now()).unwrap();
        assert!(!itineraries.is_empty());
        assert!(
            itineraries
                .iter()
                .all(|i| i.return_date == Some(date("2026-12-20")))
        );
    }

    #[test]
    fn empty_payload_means_no_flights() {
        let q = query(vec![QueryLeg::on(
            codes(&["REG"]),
            codes(&["NRT"]),
            date("2026-12-10"),
        )]);
        assert!(
            fixture("no_results")
                .itineraries(&q, Utc::now())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_payload_is_a_parse_error() {
        let reply = GoogleReply {
            url: "https://example.com".into(),
            payload: Some(serde_json::json!([null, null, [[[["XX"]]]], null])),
        };
        let q = query(vec![QueryLeg::on(
            codes(&["FCO"]),
            codes(&["NRT"]),
            date("2026-12-10"),
        )]);
        assert!(matches!(
            reply.itineraries(&q, Utc::now()),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn expands_omitted_time_components() {
        let d = serde_json::json!([2026, 12, 10]);
        assert_eq!(
            local_time(&d, &serde_json::json!([null, 31]))
                .unwrap()
                .time(),
            NaiveTime::from_hms_opt(0, 31, 0).unwrap()
        );
        assert_eq!(
            local_time(&d, &serde_json::json!([8])).unwrap().time(),
            NaiveTime::from_hms_opt(8, 0, 0).unwrap()
        );
    }
}
