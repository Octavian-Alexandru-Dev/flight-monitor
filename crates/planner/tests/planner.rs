#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{HashMap, HashSet};

use common::*;
use domain::{
    Decimal, LocationSet, ProviderQuery, QueryStats, SearchId, StayLength, TripMode, ViaHub,
};
use planner::{PlanError, Planner, Strategy, dedup};

fn planner<'a>(geo: &'a geo::GeoIndex, live: &'a domain::Capabilities) -> Planner<'a> {
    Planner {
        geo,
        live,
        scan: None,
        verify_top_n: 8,
    }
}

fn live(def: &domain::SearchDefinition) -> (Vec<ProviderQuery>, Strategy) {
    let (geo, caps) = (geo(), live_caps());
    let plan = planner(&geo, &caps)
        .live_queries(def, &[], &HashMap::new())
        .unwrap();
    (plan.queries, plan.strategy)
}

#[test]
fn single_route_single_date_is_one_query() {
    let def = search(airport("FCO"), airport("NRT"), TripMode::OneWay);
    let (queries, strategy) = live(&def);
    assert_eq!(strategy, Strategy::All);
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].legs[0].from, [iata("FCO")]);
    assert_eq!(queries[0].legs[0].date_from, date("2026-12-10"));
    assert_eq!(queries[0].max_stops, 1);
}

#[test]
fn multi_airport_sets_share_one_query_per_date() {
    let mut def = search(
        LocationSet::City("Milano".into()),
        LocationSet::City("Tokyo".into()),
        TripMode::OneWay,
    );
    window(&mut def, "2026-12-01", "2026-12-05");
    let (queries, _) = live(&def);
    assert_eq!(queries.len(), 5);
    assert!(
        queries
            .iter()
            .all(|q| q.legs[0].from.len() == 3 && q.legs[0].to.len() == 2)
    );
}

#[test]
fn country_search_is_chunked_and_capped_by_budget() {
    let mut def = search(
        airport("FCO"),
        LocationSet::Country("JP".into()),
        TripMode::OneWay,
    );
    window(&mut def, "2026-12-01", "2026-12-30");
    def.policy.query_budget = 25;
    let (geo, caps) = (geo(), live_caps());
    let plan = planner(&geo, &caps)
        .live_queries(&def, &[], &HashMap::new())
        .unwrap();
    let japan = geo
        .resolve(&LocationSet::Country("JP".into()))
        .unwrap()
        .len();
    assert_eq!(plan.candidates, 30 * japan.div_ceil(7));
    assert_eq!(plan.strategy, Strategy::Budget);
    assert_eq!(plan.queries.len(), 25);
    assert!(plan.queries.iter().all(|q| q.legs[0].to.len() <= 7));
    let unique: HashSet<_> = plan.queries.iter().collect();
    assert_eq!(unique.len(), 25);
}

#[test]
fn budget_prefers_historically_cheap_queries_and_explores_the_rest() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::OneWay);
    window(&mut def, "2026-12-01", "2026-12-20");
    def.policy.query_budget = 10;
    let (geo, caps) = (geo(), live_caps());
    let p = planner(&geo, &caps);
    let all = p
        .live_queries(
            &{
                let mut d = def.clone();
                d.policy.query_budget = 100;
                d
            },
            &[],
            &HashMap::new(),
        )
        .unwrap()
        .queries;
    let mut history = HashMap::new();
    for (i, q) in all.iter().enumerate() {
        history.insert(
            q.key(),
            QueryStats {
                best_price: Some(Decimal::from(1000 - i as i64)),
                runs: 1,
                runs_with_results: 1,
                last_run_at: None,
            },
        );
    }
    let never_run = all[0].clone();
    history.remove(&never_run.key());
    let chosen = p.live_queries(&def, &[], &history).unwrap().queries;
    assert_eq!(chosen.len(), 10);
    assert!(
        chosen.contains(&never_run),
        "unexplored queries get a share"
    );
    let cheapest = all.last().unwrap();
    assert!(chosen.contains(cheapest), "cheapest known query is kept");
    assert!(
        !chosen.contains(&all[1]),
        "expensive known query is dropped"
    );
}

#[test]
fn decoupled_round_trip_keeps_both_directions_within_budget() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::RoundTripDecoupled);
    window(&mut def, "2026-12-01", "2026-12-20");
    def.policy.query_budget = 12;
    let (queries, strategy) = live(&def);
    assert_eq!(strategy, Strategy::Budget);
    assert_eq!(queries.len(), 12);
    let outbound = queries
        .iter()
        .filter(|q| q.legs[0].from == [iata("FCO")])
        .count();
    assert!(
        (5..=7).contains(&outbound),
        "balanced split, got {outbound}"
    );
}

#[test]
fn decoupled_round_trip_counts_n_plus_m_queries() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::RoundTripDecoupled);
    window(&mut def, "2026-12-01", "2026-12-03");
    let (queries, _) = live(&def);
    // 3 outbound dates + returns from Dec 8 (1st + 7) to Dec 12 (3rd + 9).
    assert_eq!(queries.len(), 3 + 5);
}

#[test]
fn coupled_round_trip_counts_n_times_m_queries() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::RoundTripCoupled);
    window(&mut def, "2026-12-01", "2026-12-03");
    let (queries, _) = live(&def);
    assert_eq!(queries.len(), 3 * 3);
    assert!(queries.iter().all(|q| q.is_round_trip()));
    let q = &queries[0];
    assert_eq!(q.legs[1].from, q.legs[0].to);
    assert_eq!((q.legs[1].date_from - q.legs[0].date_from).num_days(), 7);
}

#[test]
fn hubs_add_both_hops() {
    let mut def = search(airport("REG"), airport("NRT"), TripMode::OneWay);
    def.via = Some(ViaHub {
        hubs: LocationSet::City("Milano".into()),
        min_connection_minutes: 120,
        max_connection_minutes: 36 * 60,
    });
    let (queries, _) = live(&def);
    let descriptions: Vec<String> = queries.iter().map(ProviderQuery::describe).collect();
    assert!(descriptions.contains(&"REG→NRT 2026-12-10".to_string()));
    assert!(descriptions.contains(&"REG→BGY,LIN,MXP 2026-12-10".to_string()));
    assert!(
        descriptions.contains(&"BGY,LIN,MXP→NRT 2026-12-11".to_string()),
        "{descriptions:?}"
    );
}

#[test]
fn companions_get_their_own_queries() {
    let mut def = search(airport("FCO"), airport("BCN"), TripMode::RoundTripDecoupled);
    def.companions = vec![domain::Companion {
        name: "Luca".into(),
        origin: LocationSet::City("Milano".into()),
    }];
    let (queries, _) = live(&def);
    let descriptions: Vec<String> = queries.iter().map(ProviderQuery::describe).collect();
    assert!(descriptions.contains(&"BGY,LIN,MXP→BCN 2026-12-10".to_string()));
    assert!(descriptions.contains(&"BCN→BGY,LIN,MXP 2026-12-17".to_string()));
    assert!(
        queries
            .iter()
            .filter(|q| q.legs[0].from.len() == 3)
            .all(|q| q.passengers == 1)
    );
}

#[test]
fn identical_queries_across_searches_run_once() {
    let a = search(airport("FCO"), airport("NRT"), TripMode::OneWay);
    let mut b = a.clone();
    b.name = "other".into();
    b.alerts.clear();
    let mut c = a.clone();
    c.constraints.max_stops = 0;
    let plans = vec![
        (SearchId(1), live(&a).0),
        (SearchId(2), live(&b).0),
        (SearchId(3), live(&c).0),
    ];
    let merged = dedup(&plans);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].1, [SearchId(1), SearchId(2)]);
    assert_eq!(merged[1].1, [SearchId(3)]);
}

#[test]
fn scan_then_verify_runs_fewer_live_queries_for_a_country() {
    let mut def = search(
        airport("FCO"),
        LocationSet::Country("JP".into()),
        TripMode::OneWay,
    );
    window(&mut def, "2026-12-01", "2026-12-30");
    let (geo, live_caps, scan_caps) = (geo(), live_caps(), scan_caps());
    let without_scan = Planner {
        geo: &geo,
        live: &live_caps,
        scan: None,
        verify_top_n: 8,
    };
    let with_scan = Planner {
        scan: Some(&scan_caps),
        ..without_scan
    };
    let japan = geo.resolve(&LocationSet::Country("JP".into())).unwrap();

    let scans = with_scan.scan_queries(&def).unwrap();
    assert_eq!(
        scans.len(),
        japan.len(),
        "one query per airport pair and month"
    );
    assert!(
        scans
            .iter()
            .all(|q| q.legs[0].date_from == date("2026-12-01")
                && q.legs[0].date_to == date("2026-12-30"))
    );

    let quotes: Vec<_> = (1..=30)
        .map(|day| quote("FCO", "NRT", &format!("2026-12-{day:02}"), 400 + day))
        .chain([quote("FCO", "OKA", "2026-12-15", 380)])
        .collect();
    let verified = with_scan
        .live_queries(&def, &quotes, &HashMap::new())
        .unwrap();
    let budget_only = without_scan
        .live_queries(&def, &[], &HashMap::new())
        .unwrap();
    assert_eq!(verified.strategy, Strategy::ScanVerify);
    assert_eq!(verified.queries.len(), 8);
    assert!(verified.queries.len() < budget_only.queries.len());
    let dates: Vec<_> = verified
        .queries
        .iter()
        .map(|q| q.legs[0].date_from)
        .collect();
    assert_eq!(
        dates[0],
        date("2026-12-15"),
        "cheapest quote first: {dates:?}"
    );
    assert!(dates.contains(&date("2026-12-01")));
    assert!(!dates.contains(&date("2026-12-30")));

    let estimate = with_scan.estimate(&def).unwrap();
    assert_eq!(estimate.live_per_cycle, 8);
    assert_eq!(estimate.scan_per_cycle, japan.len());
}

#[test]
fn small_searches_skip_the_scan() {
    let def = search(airport("FCO"), airport("NRT"), TripMode::OneWay);
    let (geo, live_caps, scan_caps) = (geo(), live_caps(), scan_caps());
    let p = Planner {
        geo: &geo,
        live: &live_caps,
        scan: Some(&scan_caps),
        verify_top_n: 8,
    };
    assert!(p.scan_queries(&def).unwrap().is_empty());
}

#[test]
fn open_destination_needs_a_scan_and_uses_its_cheapest_destinations() {
    let mut def = search(airport("FCO"), LocationSet::Anywhere, TripMode::OneWay);
    def.stay = None;
    let (geo, live_caps, scan_caps) = (geo(), live_caps(), scan_caps());
    let without = Planner {
        geo: &geo,
        live: &live_caps,
        scan: None,
        verify_top_n: 2,
    };
    assert!(matches!(
        without.scan_queries(&def),
        Err(PlanError::NeedsScanProvider)
    ));

    let with = Planner {
        scan: Some(&scan_caps),
        ..without
    };
    let scans = with.scan_queries(&def).unwrap();
    assert_eq!(scans.len(), 1);
    assert!(scans[0].legs[0].to.is_empty());

    let quotes = [
        quote("FCO", "BCN", "2026-12-10", 30),
        quote("FCO", "ATH", "2026-12-10", 45),
        quote("FCO", "JFK", "2026-12-10", 300),
    ];
    let plan = with.live_queries(&def, &quotes, &HashMap::new()).unwrap();
    assert_eq!(plan.queries.len(), 1);
    assert_eq!(plan.queries[0].legs[0].to, [iata("ATH"), iata("BCN")]);
}

#[test]
fn estimate_reports_resolved_airports() {
    let mut def = search(
        LocationSet::City("Roma".into()),
        airport("NRT"),
        TripMode::RoundTripCoupled,
    );
    def.stay = Some(StayLength {
        min_nights: 5,
        max_nights: 6,
    });
    let (geo, caps) = (geo(), live_caps());
    let estimate = planner(&geo, &caps).estimate(&def).unwrap();
    assert_eq!(estimate.origins, [iata("CIA"), iata("FCO")]);
    assert_eq!(estimate.candidates, 2);
    assert_eq!(estimate.live_per_cycle, 2);
}

#[test]
fn open_destination_round_trip_scans_outbound_and_verifies_fares() {
    let mut def = search(
        airport("FCO"),
        LocationSet::Anywhere,
        TripMode::RoundTripCoupled,
    );
    def.stay = Some(StayLength {
        min_nights: 3,
        max_nights: 3,
    });
    let (geo, live_caps, scan_caps) = (geo(), live_caps(), scan_caps());
    let p = Planner {
        geo: &geo,
        live: &live_caps,
        scan: Some(&scan_caps),
        verify_top_n: 4,
    };
    let scans = p.scan_queries(&def).unwrap();
    assert!(scans.iter().all(|q| !q.legs[0].from.is_empty()));
    assert_eq!(scans.len(), 1);

    let quotes = [
        quote("FCO", "BCN", "2026-12-10", 30),
        quote("FCO", "ATH", "2026-12-10", 45),
    ];
    let plan = p.live_queries(&def, &quotes, &HashMap::new()).unwrap();
    assert_eq!(plan.queries.len(), 1);
    let q = &plan.queries[0];
    assert!(q.is_round_trip());
    assert_eq!(q.legs[0].to, [iata("ATH"), iata("BCN")]);
    assert_eq!(q.legs[1].date_from, date("2026-12-13"));
}
