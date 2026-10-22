use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime};
use domain::{
    AlertRule, Companion, CostModel, DateSpec, Decimal, GroupGoal, HourRange, Iata,
    ItineraryConstraints, LocationSet, MonitorPolicy, SearchDefinition, StayLength, TripMode,
    ViaHub,
};
use geo::GeoIndex;
use serde::{Deserialize, Serialize};

/// The search form exactly as the browser submits it: every field is text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchForm {
    pub name: String,
    pub origin_kind: String,
    pub origin_value: String,
    pub dest_kind: String,
    pub dest_value: String,
    pub mode: String,
    pub date_from: String,
    pub date_to: String,
    pub stay_min: String,
    pub stay_max: String,
    pub return_origin_kind: String,
    pub return_origin_value: String,
    pub return_dest_kind: String,
    pub return_dest_value: String,
    pub max_stops: String,
    pub min_layover_h: String,
    pub max_layover_h: String,
    pub max_total_h: String,
    pub excluded_airlines: String,
    pub excluded_hours: String,
    pub checked_bags: String,
    pub max_price: String,
    pub passengers: String,
    pub currency: String,
    pub poll_hours: String,
    pub budget: String,
    pub adaptive: Option<String>,
    pub alert_below: String,
    pub alert_drop: String,
    pub alert_new_min: Option<String>,
    pub alert_anomaly: Option<String>,
    pub via_kind: String,
    pub via_value: String,
    pub via_min_h: String,
    pub via_max_h: String,
    pub companions: String,
    pub group_goal: String,
    pub group_spread_h: String,
    pub access_costs: String,
    pub overnight_cost: String,
}

#[derive(Clone, Copy, Debug)]
pub struct FormDefaults {
    pub budget: u32,
    pub poll_hours: u32,
}

#[derive(Debug, Default)]
pub struct FormErrors {
    pub fields: BTreeMap<&'static str, String>,
    pub general: Option<String>,
}

impl FormErrors {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.general.is_none()
    }

    pub fn summary(&self) -> String {
        if let Some(general) = &self.general {
            return general.clone();
        }
        format!("{} campi da correggere", self.fields.len())
    }
}

fn checked(on: bool) -> Option<String> {
    on.then(|| "on".to_string())
}

fn hours(minutes: Option<u32>) -> String {
    minutes.map_or_else(String::new, |m| {
        let h = Decimal::from(m) / Decimal::from(60);
        h.round_dp(2).normalize().to_string()
    })
}

fn decimal(value: Option<Decimal>) -> String {
    value.map_or_else(String::new, |d| d.normalize().to_string())
}

/// `(kind, value)` pair shown by the location pickers.
pub fn location_fields(set: &LocationSet) -> (String, String) {
    let (kind, value) = match set {
        LocationSet::Airport(code) => ("airports", code.to_string()),
        LocationSet::Airports(codes) => (
            "airports",
            codes
                .iter()
                .map(Iata::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        LocationSet::City(name) => ("city", name.clone()),
        LocationSet::Country(code) => ("country", code.clone()),
        LocationSet::Radius { lat, lon, km } => ("radius", format!("{lat}, {lon}, {km}")),
        LocationSet::Anywhere => ("anywhere", String::new()),
    };
    (kind.to_string(), value)
}

fn optional_location(set: Option<&LocationSet>) -> (String, String) {
    set.map_or_else(|| (String::new(), String::new()), location_fields)
}

impl SearchForm {
    pub fn blank(defaults: FormDefaults) -> Self {
        Self {
            origin_kind: "airports".into(),
            dest_kind: "city".into(),
            mode: "round_trip_decoupled".into(),
            stay_min: "7".into(),
            stay_max: "10".into(),
            max_stops: "1".into(),
            checked_bags: "0".into(),
            passengers: "1".into(),
            currency: "EUR".into(),
            poll_hours: defaults.poll_hours.to_string(),
            budget: defaults.budget.to_string(),
            adaptive: checked(true),
            alert_drop: "10".into(),
            alert_new_min: checked(true),
            alert_anomaly: checked(true),
            via_min_h: "3".into(),
            via_max_h: "12".into(),
            group_goal: "min_cost".into(),
            group_spread_h: "3".into(),
            ..Self::default()
        }
    }

    pub fn from_definition(def: &SearchDefinition) -> Self {
        let (origin_kind, origin_value) = location_fields(&def.origin);
        let (dest_kind, dest_value) = location_fields(&def.destination);
        let (return_origin_kind, return_origin_value) =
            optional_location(def.return_origin.as_ref());
        let (return_dest_kind, return_dest_value) =
            optional_location(def.return_destination.as_ref());
        let (via_kind, via_value) = optional_location(def.via.as_ref().map(|v| &v.hubs));
        let c = &def.constraints;
        let rule = |pick: fn(&AlertRule) -> Option<Decimal>| def.alerts.iter().find_map(pick);
        Self {
            name: def.name.clone(),
            origin_kind,
            origin_value,
            dest_kind,
            dest_value,
            mode: mode_value(def.mode).into(),
            date_from: def.outbound.first().to_string(),
            date_to: match def.outbound {
                DateSpec::Window { to, .. } => to.to_string(),
                DateSpec::Fixed { .. } => String::new(),
            },
            stay_min: def
                .stay
                .map(|s| s.min_nights.to_string())
                .unwrap_or_default(),
            stay_max: def
                .stay
                .map(|s| s.max_nights.to_string())
                .unwrap_or_default(),
            return_origin_kind,
            return_origin_value,
            return_dest_kind,
            return_dest_value,
            max_stops: c.max_stops.to_string(),
            min_layover_h: hours(c.min_layover_minutes),
            max_layover_h: hours(c.max_layover_minutes),
            max_total_h: hours(c.max_total_minutes),
            excluded_airlines: c.excluded_airlines.join(", "),
            excluded_hours: c
                .excluded_departure_hours
                .iter()
                .map(|r| format!("{}-{}", r.from.format("%H:%M"), r.to.format("%H:%M")))
                .collect::<Vec<_>>()
                .join(", "),
            checked_bags: c.checked_bags.to_string(),
            max_price: decimal(c.max_price),
            passengers: def.passengers.to_string(),
            currency: def.currency.clone(),
            poll_hours: hours(Some(def.policy.poll_interval_minutes)),
            budget: def.policy.query_budget.to_string(),
            adaptive: checked(def.policy.adaptive),
            alert_below: decimal(rule(|r| match r {
                AlertRule::BelowPrice(p) => Some(*p),
                _ => None,
            })),
            alert_drop: decimal(rule(|r| match r {
                AlertRule::DropPercent(p) => Some(*p),
                _ => None,
            })),
            alert_new_min: checked(def.alerts.contains(&AlertRule::NewHistoricMinimum)),
            alert_anomaly: checked(def.alerts.contains(&AlertRule::PriceAnomaly)),
            via_kind,
            via_value,
            via_min_h: hours(def.via.as_ref().map(|v| v.min_connection_minutes)),
            via_max_h: hours(def.via.as_ref().map(|v| v.max_connection_minutes)),
            companions: def
                .companions
                .iter()
                .map(|c| format!("{}: {}", c.name, location_fields(&c.origin).1))
                .collect::<Vec<_>>()
                .join("\n"),
            group_goal: match def.group_goal {
                GroupGoal::MinTotalCost => "min_cost".into(),
                GroupGoal::AlignArrivals { .. } => "align".into(),
            },
            group_spread_h: match def.group_goal {
                GroupGoal::AlignArrivals { max_spread_minutes } => hours(Some(max_spread_minutes)),
                GroupGoal::MinTotalCost => "3".into(),
            },
            access_costs: def
                .costs
                .airport_access
                .iter()
                .map(|(code, cost)| format!("{code}={}", cost.normalize()))
                .collect::<Vec<_>>()
                .join(", "),
            overnight_cost: decimal(def.costs.overnight_layover),
        }
    }

    /// Parses and validates the form; errors are keyed by field name.
    pub fn to_definition(&self, geo: &GeoIndex) -> Result<SearchDefinition, FormErrors> {
        let mut p = Parser {
            geo,
            errors: FormErrors::default(),
        };
        let mode = match self.mode.as_str() {
            "one_way" => TripMode::OneWay,
            "round_trip_coupled" => TripMode::RoundTripCoupled,
            _ => TripMode::RoundTripDecoupled,
        };
        let origin = p.location("origin_value", &self.origin_kind, &self.origin_value);
        let destination = p.location("dest_value", &self.dest_kind, &self.dest_value);
        let return_origin = p.optional_location(
            "return_origin_value",
            &self.return_origin_kind,
            &self.return_origin_value,
        );
        let return_destination = p.optional_location(
            "return_dest_value",
            &self.return_dest_kind,
            &self.return_dest_value,
        );
        let from = p.date("date_from", &self.date_from);
        let to = if self.date_to.trim().is_empty() {
            from
        } else {
            p.date("date_to", &self.date_to)
        };
        let stay = if mode.is_round_trip() {
            let min = p.number::<u16>("stay_min", &self.stay_min);
            let max = p.number::<u16>("stay_max", &self.stay_max);
            min.zip(max).map(|(min_nights, max_nights)| StayLength {
                min_nights,
                max_nights,
            })
        } else {
            None
        };
        let constraints = ItineraryConstraints {
            max_stops: p.number("max_stops", &self.max_stops).unwrap_or(1),
            min_layover_minutes: p.hours("min_layover_h", &self.min_layover_h),
            max_layover_minutes: p.hours("max_layover_h", &self.max_layover_h),
            max_total_minutes: p.hours("max_total_h", &self.max_total_h),
            excluded_airlines: p.airlines(&self.excluded_airlines),
            excluded_departure_hours: p.hour_ranges(&self.excluded_hours),
            checked_bags: p
                .number("checked_bags", or_zero(&self.checked_bags))
                .unwrap_or(0),
            max_price: p.optional_decimal("max_price", &self.max_price),
        };
        let policy = MonitorPolicy {
            poll_interval_minutes: p.hours("poll_hours", &self.poll_hours).unwrap_or(0),
            query_budget: p.number("budget", &self.budget).unwrap_or(0),
            adaptive: self.adaptive.is_some(),
        };
        let mut alerts = Vec::new();
        if let Some(price) = p.optional_decimal("alert_below", &self.alert_below) {
            alerts.push(AlertRule::BelowPrice(price));
        }
        if let Some(percent) = p.optional_decimal("alert_drop", &self.alert_drop) {
            alerts.push(AlertRule::DropPercent(percent));
        }
        if self.alert_new_min.is_some() {
            alerts.push(AlertRule::NewHistoricMinimum);
        }
        if self.alert_anomaly.is_some() {
            alerts.push(AlertRule::PriceAnomaly);
        }
        let via = p
            .optional_location("via_value", &self.via_kind, &self.via_value)
            .map(|hubs| ViaHub {
                hubs,
                min_connection_minutes: p.hours("via_min_h", &self.via_min_h).unwrap_or(120),
                max_connection_minutes: p.hours("via_max_h", &self.via_max_h).unwrap_or(720),
            });
        let companions = p.companions(&self.companions);
        let group_goal = if self.group_goal == "align" {
            GroupGoal::AlignArrivals {
                max_spread_minutes: p
                    .hours("group_spread_h", &self.group_spread_h)
                    .unwrap_or(180),
            }
        } else {
            GroupGoal::MinTotalCost
        };
        let costs = CostModel {
            airport_access: p.access_costs(&self.access_costs),
            overnight_layover: p.optional_decimal("overnight_cost", &self.overnight_cost),
        };
        let passengers = p.number("passengers", &self.passengers).unwrap_or(1);

        if self.name.trim().is_empty() {
            p.fail("name", "Dai un nome alla ricerca");
        }
        let (Some(origin), Some(destination), Some(from), Some(to)) =
            (origin, destination, from, to)
        else {
            return Err(p.errors);
        };
        let definition = SearchDefinition {
            name: self.name.trim().to_string(),
            origin,
            destination,
            mode,
            outbound: if from == to {
                DateSpec::Fixed { date: from }
            } else {
                DateSpec::Window { from, to }
            },
            stay,
            return_origin,
            return_destination,
            constraints,
            passengers,
            currency: self.currency.trim().to_ascii_uppercase(),
            policy,
            alerts,
            via,
            companions,
            group_goal,
            costs,
        };
        if !p.errors.is_empty() {
            return Err(p.errors);
        }
        definition.validate().map_err(|e| FormErrors {
            fields: BTreeMap::new(),
            general: Some(e.to_string().replace("invalid search: ", "")),
        })?;
        Ok(definition)
    }
}

fn or_zero(value: &str) -> &str {
    if value.trim().is_empty() { "0" } else { value }
}

pub fn mode_value(mode: TripMode) -> &'static str {
    match mode {
        TripMode::OneWay => "one_way",
        TripMode::RoundTripCoupled => "round_trip_coupled",
        TripMode::RoundTripDecoupled => "round_trip_decoupled",
    }
}

struct Parser<'a> {
    geo: &'a GeoIndex,
    errors: FormErrors,
}

impl Parser<'_> {
    fn fail(&mut self, field: &'static str, message: &str) {
        self.errors
            .fields
            .entry(field)
            .or_insert_with(|| message.to_string());
    }

    fn date(&mut self, field: &'static str, value: &str) -> Option<NaiveDate> {
        match value.trim().parse() {
            Ok(date) => Some(date),
            Err(_) => {
                self.fail(field, "Inserisci una data valida");
                None
            }
        }
    }

    fn number<T: std::str::FromStr>(&mut self, field: &'static str, value: &str) -> Option<T> {
        match value.trim().parse() {
            Ok(n) => Some(n),
            Err(_) => {
                self.fail(field, "Inserisci un numero intero");
                None
            }
        }
    }

    fn optional_decimal(&mut self, field: &'static str, value: &str) -> Option<Decimal> {
        let value = value.trim().replace(',', ".");
        if value.is_empty() {
            return None;
        }
        match value.parse::<Decimal>() {
            Ok(d) if d >= Decimal::ZERO => Some(d),
            _ => {
                self.fail(field, "Inserisci un importo valido");
                None
            }
        }
    }

    /// Accepts `1.5`, `1,5` or `1:30`.
    fn hours(&mut self, field: &'static str, value: &str) -> Option<u32> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let minutes = match value.split_once(':') {
            Some((h, m)) => h
                .parse::<u32>()
                .ok()
                .zip(m.parse::<u32>().ok())
                .map(|(h, m)| h * 60 + m),
            None => value
                .replace(',', ".")
                .parse::<Decimal>()
                .ok()
                .filter(|d| *d >= Decimal::ZERO)
                .and_then(|d| (d * Decimal::from(60)).round().to_string().parse().ok()),
        };
        if minutes.is_none() {
            self.fail(field, "Usa ore come 2, 1.5 o 1:30");
        }
        minutes
    }

    fn location(&mut self, field: &'static str, kind: &str, value: &str) -> Option<LocationSet> {
        let parsed = parse_location(kind, value, self.geo);
        match parsed {
            Ok(Some(set)) => match self.geo.resolve(&set) {
                Ok(_) | Err(geo::GeoError::OpenDestination) => Some(set),
                Err(error) => {
                    self.fail(field, &capitalize(&error.to_string()));
                    None
                }
            },
            Ok(None) => {
                self.fail(field, "Campo obbligatorio");
                None
            }
            Err(message) => {
                self.fail(field, &message);
                None
            }
        }
    }

    fn optional_location(
        &mut self,
        field: &'static str,
        kind: &str,
        value: &str,
    ) -> Option<LocationSet> {
        if kind.is_empty() || kind == "same" || (kind != "anywhere" && value.trim().is_empty()) {
            return None;
        }
        self.location(field, kind, value)
    }

    fn airlines(&mut self, value: &str) -> Vec<String> {
        let codes: Vec<String> = split_list(value).map(str::to_ascii_uppercase).collect();
        if codes.iter().any(|c| c.len() != 2) {
            self.fail(
                "excluded_airlines",
                "Usa codici compagnia di 2 caratteri (es. FR, W6)",
            );
        }
        codes
    }

    fn hour_ranges(&mut self, value: &str) -> Vec<HourRange> {
        let mut ranges = Vec::new();
        for part in split_list(value) {
            let parsed = part.split_once('-').and_then(|(a, b)| {
                let time = |t: &str| NaiveTime::parse_from_str(t.trim(), "%H:%M").ok();
                time(a).zip(time(b))
            });
            match parsed {
                Some((from, to)) => ranges.push(HourRange { from, to }),
                None => self.fail("excluded_hours", "Usa fasce come 00:00-06:00"),
            }
        }
        ranges
    }

    fn companions(&mut self, value: &str) -> Vec<Companion> {
        let mut companions = Vec::new();
        for line in value.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let Some((name, place)) = line.split_once(':') else {
                self.fail(
                    "companions",
                    "Una riga per persona: «Nome: città o aeroporti»",
                );
                continue;
            };
            let origin = infer_location(place, self.geo);
            match self.geo.resolve(&origin) {
                Ok(_) => companions.push(Companion {
                    name: name.trim().to_string(),
                    origin,
                }),
                Err(error) => self.fail("companions", &capitalize(&error.to_string())),
            }
        }
        companions
    }

    fn access_costs(&mut self, value: &str) -> BTreeMap<Iata, Decimal> {
        let mut costs = BTreeMap::new();
        for part in split_list(value) {
            let parsed = part.split_once(['=', ' ']).and_then(|(code, cost)| {
                Iata::new(code)
                    .ok()
                    .zip(cost.trim().replace(',', ".").parse::<Decimal>().ok())
            });
            match parsed {
                Some((code, cost)) => {
                    costs.insert(code, cost);
                }
                None => self.fail("access_costs", "Usa coppie come FCO=25, BGY=40"),
            }
        }
        costs
    }
}

fn split_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Parses a location picker value; `Ok(None)` when it is empty.
pub fn parse_location(
    kind: &str,
    value: &str,
    geo: &GeoIndex,
) -> Result<Option<LocationSet>, String> {
    let value = value.trim();
    if kind == "anywhere" {
        return Ok(Some(LocationSet::Anywhere));
    }
    if value.is_empty() {
        return Ok(None);
    }
    match kind {
        "airports" => {
            let codes = value
                .split([',', ';', ' '])
                .filter(|s| !s.is_empty())
                .map(Iata::new)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "Usa codici IATA di 3 lettere separati da virgola".to_string())?;
            Ok(Some(match codes.as_slice() {
                [single] => LocationSet::Airport(single.clone()),
                _ => LocationSet::Airports(codes),
            }))
        }
        "city" => Ok(Some(LocationSet::City(value.to_string()))),
        "country" => geo
            .country_code(value)
            .map(|code| Some(LocationSet::Country(code)))
            .ok_or_else(|| format!("Paese sconosciuto: «{value}»")),
        "radius" => parse_radius(value, geo).map(Some),
        _ => Err("Tipo di luogo non valido".into()),
    }
}

/// `lat, lon, km` or `IATA km` (radius around an airport).
fn parse_radius(value: &str, geo: &GeoIndex) -> Result<LocationSet, String> {
    let parts: Vec<&str> = value
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let number = |s: &str| s.parse::<f64>().ok();
    match parts.as_slice() {
        [code, km] => {
            let airport = Iata::new(code)
                .ok()
                .and_then(|c| geo.airport(&c).cloned())
                .ok_or_else(|| format!("Aeroporto sconosciuto: «{code}»"))?;
            let km = number(km).ok_or("Indica il raggio in km")?;
            Ok(LocationSet::Radius {
                lat: airport.lat,
                lon: airport.lon,
                km,
            })
        }
        [lat, lon, km] => match (number(lat), number(lon), number(km)) {
            (Some(lat), Some(lon), Some(km)) if km > 0.0 => {
                Ok(LocationSet::Radius { lat, lon, km })
            }
            _ => Err("Usa «lat, lon, km» oppure «REG 120»".into()),
        },
        _ => Err("Usa «lat, lon, km» oppure «REG 120»".into()),
    }
}

/// Guesses the location kind from free text (used for companions).
pub fn infer_location(value: &str, geo: &GeoIndex) -> LocationSet {
    let value = value.trim();
    let tokens: Vec<&str> = value.split([',', ' ']).filter(|s| !s.is_empty()).collect();
    let all_codes = !tokens.is_empty()
        && tokens
            .iter()
            .all(|t| t.len() == 3 && t.chars().all(|c| c.is_ascii_uppercase()));
    if all_codes && let Ok(Some(set)) = parse_location("airports", value, geo) {
        return set;
    }
    if value.len() == 2
        && let Some(code) = geo.country_code(value)
    {
        return LocationSet::Country(code);
    }
    LocationSet::City(value.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn geo() -> GeoIndex {
        GeoIndex::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).unwrap()
    }

    fn filled() -> SearchForm {
        SearchForm {
            name: "Giappone".into(),
            origin_value: "REG, SUF, CTA".into(),
            dest_kind: "country".into(),
            dest_value: "Japan".into(),
            date_from: "2026-12-01".into(),
            date_to: "2026-12-10".into(),
            min_layover_h: "1:30".into(),
            max_layover_h: "8".into(),
            excluded_airlines: "fr, w6".into(),
            excluded_hours: "00:00-06:00".into(),
            alert_below: "650".into(),
            via_kind: "city".into(),
            via_value: "Milano".into(),
            companions: "Giulia: Torino\nLuca: BRI".into(),
            group_goal: "align".into(),
            access_costs: "REG=10, CTA 35".into(),
            overnight_cost: "60".into(),
            ..SearchForm::blank(FormDefaults {
                budget: 40,
                poll_hours: 6,
            })
        }
    }

    #[test]
    fn parses_a_complete_form() {
        let def = filled().to_definition(&geo()).unwrap();
        assert_eq!(def.destination, LocationSet::Country("JP".into()));
        assert_eq!(def.constraints.min_layover_minutes, Some(90));
        assert_eq!(def.constraints.excluded_airlines, ["FR", "W6"]);
        assert_eq!(def.policy.poll_interval_minutes, 360);
        assert!(
            def.alerts
                .contains(&AlertRule::BelowPrice(Decimal::from(650)))
        );
        assert_eq!(def.companions.len(), 2);
        assert_eq!(
            def.companions[1].origin,
            LocationSet::Airport(Iata::new("BRI").unwrap())
        );
        assert_eq!(
            def.group_goal,
            GroupGoal::AlignArrivals {
                max_spread_minutes: 180
            }
        );
        assert_eq!(def.costs.airport_access.len(), 2);
    }

    #[test]
    fn round_trips_through_the_form() {
        let geo = geo();
        let def = filled().to_definition(&geo).unwrap();
        let again = SearchForm::from_definition(&def)
            .to_definition(&geo)
            .unwrap();
        assert_eq!(again, def);
    }

    #[test]
    fn reports_errors_per_field() {
        let form = SearchForm {
            origin_value: "XYZ".into(),
            dest_value: "Atlantide".into(),
            date_from: "domani".into(),
            excluded_hours: "notte".into(),
            ..filled()
        };
        let errors = form.to_definition(&geo()).unwrap_err();
        for field in ["origin_value", "dest_value", "date_from", "excluded_hours"] {
            assert!(errors.fields.contains_key(field), "{field}: {errors:?}");
        }
    }

    #[test]
    fn domain_rules_surface_as_general_error() {
        let form = SearchForm {
            mode: "round_trip_coupled".into(),
            ..filled()
        };
        let errors = form.to_definition(&geo()).unwrap_err();
        assert!(errors.general.is_some());
    }

    #[test]
    fn radius_around_an_airport() {
        let set = parse_location("radius", "REG 120", &geo())
            .unwrap()
            .unwrap();
        assert!(matches!(set, LocationSet::Radius { km, .. } if km == 120.0));
    }
}
