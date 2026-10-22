#![allow(clippy::unwrap_used)]

use std::path::Path;

use domain::{Iata, LocationSet};
use geo::{GeoError, GeoIndex, SuggestionKind};

fn index() -> GeoIndex {
    GeoIndex::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).unwrap()
}

fn codes(list: &[Iata]) -> Vec<&str> {
    list.iter().map(Iata::as_str).collect()
}

#[test]
fn resolves_country_to_major_airports() {
    let japan = index().resolve(&LocationSet::Country("jp".into())).unwrap();
    let japan = codes(&japan);
    for hub in ["NRT", "HND", "KIX", "CTS", "FUK"] {
        assert!(japan.contains(&hub), "{hub} missing from JP");
    }
    assert!(japan.len() > 30);
    assert!(!japan.contains(&"FCO"));
}

#[test]
fn resolves_multi_airport_city_in_any_language() {
    let geo = index();
    for name in ["Milano", "milan", "MIL"] {
        let milan = geo.resolve(&LocationSet::City(name.into())).unwrap();
        assert_eq!(codes(&milan), ["BGY", "LIN", "MXP"], "{name}");
    }
}

#[test]
fn resolves_city_without_metro_entry() {
    let catania = index()
        .resolve(&LocationSet::City("Catania".into()))
        .unwrap();
    assert_eq!(codes(&catania), ["CTA"]);
}

#[test]
fn resolves_radius() {
    let around_reggio = index()
        .resolve(&LocationSet::Radius {
            lat: 38.07,
            lon: 15.65,
            km: 110.0,
        })
        .unwrap();
    let around_reggio = codes(&around_reggio);
    for code in ["REG", "SUF", "CTA"] {
        assert!(around_reggio.contains(&code), "{code} missing");
    }
    assert!(!around_reggio.contains(&"FCO"));
}

#[test]
fn rejects_unknown_locations() {
    let geo = index();
    assert!(matches!(
        geo.resolve(&LocationSet::Airport(Iata::new("ZZZ").unwrap())),
        Err(GeoError::UnknownAirport(_))
    ));
    assert!(matches!(
        geo.resolve(&LocationSet::City("Atlantide".into())),
        Err(GeoError::UnknownCity(_))
    ));
    assert!(matches!(
        geo.resolve(&LocationSet::Anywhere),
        Err(GeoError::OpenDestination)
    ));
}

#[test]
fn knows_airports_sharing_a_city() {
    let geo = index();
    let code = |c| Iata::new(c).unwrap();
    assert!(geo.same_city(&code("LIN"), &code("MXP")));
    assert!(!geo.same_city(&code("FCO"), &code("MXP")));
}

#[test]
fn suggests_metros_airports_and_countries() {
    let geo = index();
    let suggestions = geo.suggest("mila", 5);
    assert_eq!(suggestions[0].kind, SuggestionKind::City);
    assert_eq!(suggestions[0].value, "Milano");
    assert_eq!(geo.suggest("fco", 3)[0].value, "FCO");
    assert!(geo.suggest("japa", 5).iter().any(|s| s.value == "JP"));
}
