#![allow(clippy::unwrap_used)]

mod common;

use chrono::NaiveTime;
use common::*;
use domain::{
    Companion, Decimal, Direction, GroupGoal, HourRange, LocationSet, SearchDefinition, StayLength,
    TripMode, ViaHub, Warning,
};
use planner::{build_options, resolve};

fn options(def: &SearchDefinition, itineraries: &[domain::Itinerary]) -> planner::Outcome {
    let geo = geo();
    let resolved = resolve(def, &geo).unwrap();
    build_options(def, &resolved, itineraries, &geo)
}

fn via_auh(price: i64, layover_end: &str) -> domain::Itinerary {
    trip(
        &[
            ("FCO", "AUH", "2026-12-10T10:00", "2026-12-10T19:00"),
            ("AUH", "NRT", layover_end, "2026-12-11T12:00"),
        ],
        price,
        "EY",
    )
}

#[test]
fn one_way_options_are_sorted_and_filtered() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::OneWay);
    def.constraints.min_layover_minutes = Some(60);
    def.constraints.max_layover_minutes = Some(5 * 60);
    def.constraints.excluded_airlines = vec!["su".into()];
    let direct = trip(
        &[("FCO", "NRT", "2026-12-10T13:00", "2026-12-11T09:00")],
        900,
        "AZ",
    );
    let short = via_auh(400, "2026-12-10T19:30");
    let good = via_auh(500, "2026-12-10T21:00");
    let long = via_auh(450, "2026-12-11T02:00");
    let banned = trip(
        &[("FCO", "NRT", "2026-12-10T13:00", "2026-12-11T09:00")],
        300,
        "SU",
    );
    let wrong_day = trip(
        &[("FCO", "NRT", "2026-12-12T13:00", "2026-12-13T09:00")],
        100,
        "AZ",
    );

    let outcome = options(&def, &[direct, short, good, long, banned, wrong_day]);
    let prices: Vec<Decimal> = outcome.options.iter().map(|o| o.total_price).collect();
    assert_eq!(prices, [Decimal::from(500), Decimal::from(900)]);
    assert_eq!(outcome.calendar.len(), 1);
    assert_eq!(outcome.calendar[0].price, Decimal::from(500));
}

#[test]
fn excluded_departure_hours_and_max_price() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::OneWay);
    def.constraints.excluded_departure_hours = vec![HourRange {
        from: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        to: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
    }];
    def.constraints.max_price = Some(Decimal::from(800));
    let dawn = trip(
        &[("FCO", "NRT", "2026-12-10T06:00", "2026-12-11T02:00")],
        300,
        "AZ",
    );
    let noon = trip(
        &[("FCO", "NRT", "2026-12-10T12:00", "2026-12-11T08:00")],
        600,
        "AZ",
    );
    let pricey = trip(
        &[("FCO", "NRT", "2026-12-10T15:00", "2026-12-11T11:00")],
        900,
        "AZ",
    );
    let outcome = options(&def, &[dawn, noon, pricey]);
    assert_eq!(outcome.options.len(), 1);
    assert_eq!(outcome.options[0].total_price, Decimal::from(600));
}

#[test]
fn decoupled_round_trip_pairs_respecting_stay() {
    let def = search(airport("FCO"), airport("NRT"), TripMode::RoundTripDecoupled);
    let out = trip(
        &[("FCO", "NRT", "2026-12-10T12:00", "2026-12-11T08:00")],
        500,
        "AZ",
    );
    let back_ok = trip(
        &[("NRT", "FCO", "2026-12-18T10:00", "2026-12-18T18:00")],
        400,
        "EY",
    );
    let back_short = trip(
        &[("NRT", "FCO", "2026-12-12T10:00", "2026-12-12T18:00")],
        100,
        "EY",
    );
    let back_long = trip(
        &[("NRT", "FCO", "2026-12-25T10:00", "2026-12-25T18:00")],
        100,
        "EY",
    );
    let outcome = options(&def, &[out, back_ok, back_short, back_long]);
    assert_eq!(outcome.options.len(), 1);
    let option = &outcome.options[0];
    assert_eq!(option.total_price, Decimal::from(900));
    assert_eq!(option.tickets.len(), 2);
    assert_eq!(option.tickets[1].direction, Direction::Return);
    assert_eq!(option.return_date(), Some(date("2026-12-18")));
    assert_eq!(outcome.calendar.len(), 1);
    assert_eq!(outcome.calendar[0].inbound, Some(date("2026-12-18")));
}

#[test]
fn open_jaw_returns_from_another_city() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::RoundTripDecoupled);
    def.return_origin = Some(LocationSet::City("Osaka".into()));
    let out = trip(
        &[("FCO", "NRT", "2026-12-10T12:00", "2026-12-11T08:00")],
        500,
        "AZ",
    );
    let from_osaka = trip(
        &[("KIX", "FCO", "2026-12-18T10:00", "2026-12-18T18:00")],
        450,
        "EY",
    );
    let from_tokyo = trip(
        &[("NRT", "FCO", "2026-12-18T10:00", "2026-12-18T18:00")],
        300,
        "EY",
    );
    let outcome = options(&def, &[out, from_osaka, from_tokyo]);
    assert_eq!(outcome.options.len(), 1);
    assert_eq!(
        outcome.options[0].tickets[1].itinerary.origin(),
        &iata("KIX")
    );
}

#[test]
fn coupled_round_trip_uses_fare_return_date() {
    let def = search(airport("FCO"), airport("NRT"), TripMode::RoundTripCoupled);
    let mut fare = trip(
        &[("FCO", "NRT", "2026-12-10T12:00", "2026-12-11T08:00")],
        800,
        "AZ",
    );
    fare.return_date = Some(date("2026-12-18"));
    let mut too_long = fare.clone();
    too_long.return_date = Some(date("2026-12-30"));
    let one_way = trip(
        &[("FCO", "NRT", "2026-12-10T15:00", "2026-12-11T11:00")],
        100,
        "AZ",
    );
    let outcome = options(&def, &[fare, too_long, one_way]);
    assert_eq!(outcome.options.len(), 1);
    assert_eq!(outcome.options[0].return_date(), Some(date("2026-12-18")));
    assert_eq!(outcome.calendar[0].inbound, Some(date("2026-12-18")));
}

#[test]
fn self_transfer_via_hub_flags_risks() {
    let mut def = search(airport("REG"), airport("NRT"), TripMode::OneWay);
    def.via = Some(ViaHub {
        hubs: LocationSet::City("Milano".into()),
        min_connection_minutes: 180,
        max_connection_minutes: 12 * 60,
    });
    let to_hub = trip(
        &[("REG", "LIN", "2026-12-10T07:00", "2026-12-10T08:40")],
        40,
        "AZ",
    );
    let from_hub = trip(
        &[("MXP", "NRT", "2026-12-10T13:00", "2026-12-11T09:00")],
        450,
        "NH",
    );
    let too_tight = trip(
        &[("LIN", "NRT", "2026-12-10T09:30", "2026-12-11T05:00")],
        300,
        "NH",
    );
    let outcome = options(&def, &[to_hub, from_hub, too_tight]);
    assert_eq!(outcome.options.len(), 1);
    let option = &outcome.options[0];
    assert_eq!(option.total_price, Decimal::from(490));
    assert_eq!(option.tickets.len(), 2);
    assert!(option.warnings.contains(&Warning::SelfTransfer {
        hub: iata("LIN"),
        minutes: 260
    }));
    assert!(option.warnings.contains(&Warning::AirportChange {
        from: iata("LIN"),
        to: iata("MXP")
    }));
}

#[test]
fn long_hub_connection_is_a_stopover_with_hotel_cost() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::OneWay);
    def.via = Some(ViaHub {
        hubs: airport("IST"),
        min_connection_minutes: 24 * 60,
        max_connection_minutes: 48 * 60,
    });
    def.costs.overnight_layover = Some(Decimal::from(60));
    def.costs
        .airport_access
        .insert(iata("FCO"), Decimal::from(15));
    let leg1 = trip(
        &[("FCO", "IST", "2026-12-10T08:00", "2026-12-10T12:00")],
        80,
        "TK",
    );
    let leg2 = trip(
        &[("IST", "NRT", "2026-12-11T20:00", "2026-12-12T14:00")],
        420,
        "TK",
    );
    let outcome = options(&def, &[leg1, leg2]);
    let option = &outcome.options[0];
    assert!(option.warnings.contains(&Warning::Stopover {
        hub: iata("IST"),
        hours: 32
    }));
    assert_eq!(option.extra_cost, Decimal::from(15 + 60));
    assert_eq!(option.door_to_door(), Decimal::from(575));
}

#[test]
fn group_search_aligns_arrivals_at_common_destination() {
    let mut def = search(
        airport("FCO"),
        LocationSet::City("Tokyo".into()),
        TripMode::OneWay,
    );
    def.companions = vec![Companion {
        name: "Giulia".into(),
        origin: LocationSet::City("Milano".into()),
    }];
    def.group_goal = GroupGoal::AlignArrivals {
        max_spread_minutes: 180,
    };
    let me = trip(
        &[("FCO", "NRT", "2026-12-10T12:00", "2026-12-11T08:00")],
        500,
        "AZ",
    );
    let her_cheap_late = trip(
        &[("MXP", "HND", "2026-12-10T20:00", "2026-12-11T18:00")],
        300,
        "NH",
    );
    let her_aligned = trip(
        &[("LIN", "HND", "2026-12-10T13:00", "2026-12-11T09:00")],
        420,
        "NH",
    );
    let outcome = options(&def, &[me, her_cheap_late, her_aligned]);
    let group: Vec<_> = outcome
        .options
        .iter()
        .filter(|o| o.travelers().len() == 2)
        .collect();
    assert_eq!(group.len(), 1);
    assert_eq!(group[0].total_price, Decimal::from(920));
    let hers = group[0].tickets_of(Some("Giulia"));
    assert_eq!(hers[0].itinerary.origin(), &iata("LIN"));
}

#[test]
fn group_search_minimises_total_cost_by_default() {
    let mut def = search(
        airport("FCO"),
        LocationSet::City("Tokyo".into()),
        TripMode::OneWay,
    );
    def.companions = vec![Companion {
        name: "Giulia".into(),
        origin: LocationSet::City("Milano".into()),
    }];
    let me = trip(
        &[("FCO", "NRT", "2026-12-10T12:00", "2026-12-11T08:00")],
        500,
        "AZ",
    );
    let her_cheap = trip(
        &[("MXP", "HND", "2026-12-10T20:00", "2026-12-11T18:00")],
        300,
        "NH",
    );
    let her_aligned = trip(
        &[("LIN", "HND", "2026-12-10T13:00", "2026-12-11T09:00")],
        420,
        "NH",
    );
    let outcome = options(&def, &[me, her_cheap, her_aligned]);
    assert_eq!(outcome.options[0].total_price, Decimal::from(800));
    assert!(
        outcome.options[0]
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::ArrivalSpread { minutes: 600 }))
    );
}

#[test]
fn decoupled_calendar_covers_all_date_pairs() {
    let mut def = search(airport("FCO"), airport("NRT"), TripMode::RoundTripDecoupled);
    window(&mut def, "2026-12-10", "2026-12-11");
    def.stay = Some(StayLength {
        min_nights: 7,
        max_nights: 8,
    });
    let outs = [("2026-12-10", 500), ("2026-12-11", 450)];
    let backs = [
        ("2026-12-17", 300),
        ("2026-12-18", 350),
        ("2026-12-19", 320),
    ];
    let mut all = Vec::new();
    for (day, price) in outs {
        all.push(trip(
            &[(
                "FCO",
                "NRT",
                &format!("{day}T12:00"),
                &format!("{day}T23:00"),
            )],
            price,
            "AZ",
        ));
    }
    for (day, price) in backs {
        all.push(trip(
            &[(
                "NRT",
                "FCO",
                &format!("{day}T12:00"),
                &format!("{day}T20:00"),
            )],
            price,
            "AZ",
        ));
    }
    let outcome = options(&def, &all);
    assert_eq!(outcome.calendar.len(), 4);
    let cheapest = outcome.calendar.iter().map(|c| c.price).min().unwrap();
    assert_eq!(cheapest, Decimal::from(770));
}

#[test]
fn group_calendar_shows_group_totals() {
    let mut def = search(airport("FCO"), airport("BCN"), TripMode::RoundTripDecoupled);
    def.stay = Some(StayLength {
        min_nights: 2,
        max_nights: 2,
    });
    def.companions = vec![Companion {
        name: "Giulia".into(),
        origin: LocationSet::City("Milano".into()),
    }];
    let itineraries = [
        trip(
            &[("FCO", "BCN", "2026-12-10T10:00", "2026-12-10T12:00")],
            40,
            "VY",
        ),
        trip(
            &[("BCN", "FCO", "2026-12-12T10:00", "2026-12-12T12:00")],
            30,
            "VY",
        ),
        trip(
            &[("MXP", "BCN", "2026-12-10T11:00", "2026-12-10T12:30")],
            50,
            "FR",
        ),
        trip(
            &[("BCN", "MXP", "2026-12-12T15:00", "2026-12-12T16:30")],
            20,
            "FR",
        ),
    ];
    let outcome = options(&def, &itineraries);
    assert_eq!(outcome.options[0].total_price, Decimal::from(140));
    assert_eq!(outcome.calendar.len(), 1);
    assert_eq!(outcome.calendar[0].price, Decimal::from(140));
}
