use std::collections::BTreeMap;

use chrono::{Days, NaiveDate, NaiveTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{DomainError, Iata, LocationSet};

const MAX_WINDOW_DAYS: u64 = 330;
const MIN_POLL_MINUTES: u32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SearchId(pub i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TripMode {
    OneWay,
    RoundTripCoupled,
    RoundTripDecoupled,
}

impl TripMode {
    pub fn is_round_trip(self) -> bool {
        !matches!(self, Self::OneWay)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OneWay => "Solo andata",
            Self::RoundTripCoupled => "A/R abbinato",
            Self::RoundTripDecoupled => "A/R libero",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DateSpec {
    Fixed { date: NaiveDate },
    Window { from: NaiveDate, to: NaiveDate },
}

impl DateSpec {
    pub fn first(&self) -> NaiveDate {
        match self {
            Self::Fixed { date } => *date,
            Self::Window { from, .. } => *from,
        }
    }

    pub fn last(&self) -> NaiveDate {
        match self {
            Self::Fixed { date } => *date,
            Self::Window { to, .. } => *to,
        }
    }

    pub fn dates(&self) -> Vec<NaiveDate> {
        self.first()
            .iter_days()
            .take_while(|d| *d <= self.last())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StayLength {
    pub min_nights: u16,
    pub max_nights: u16,
}

impl StayLength {
    pub fn contains(&self, nights: i64) -> bool {
        nights >= i64::from(self.min_nights) && nights <= i64::from(self.max_nights)
    }
}

/// Departure time window; `from > to` wraps around midnight (e.g. 22:00–06:00).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourRange {
    pub from: NaiveTime,
    pub to: NaiveTime,
}

impl HourRange {
    pub fn contains(&self, time: NaiveTime) -> bool {
        if self.from <= self.to {
            time >= self.from && time < self.to
        } else {
            time >= self.from || time < self.to
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItineraryConstraints {
    pub max_stops: u8,
    pub min_layover_minutes: Option<u32>,
    pub max_layover_minutes: Option<u32>,
    pub max_total_minutes: Option<u32>,
    /// Airline IATA codes (e.g. `FR`).
    pub excluded_airlines: Vec<String>,
    pub excluded_departure_hours: Vec<HourRange>,
    /// Checked bags whose fees the source should include in the price.
    pub checked_bags: u8,
    pub max_price: Option<Decimal>,
}

impl Default for ItineraryConstraints {
    fn default() -> Self {
        Self {
            max_stops: 1,
            min_layover_minutes: None,
            max_layover_minutes: None,
            max_total_minutes: None,
            excluded_airlines: Vec::new(),
            excluded_departure_hours: Vec::new(),
            checked_bags: 0,
            max_price: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AlertRule {
    BelowPrice(Decimal),
    DropPercent(Decimal),
    NewHistoricMinimum,
    /// Price far below the historical average of the route.
    PriceAnomaly,
}

/// Separate tickets joined at a hub: a self-transfer with a short connection,
/// or a deliberate stopover with a long one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViaHub {
    pub hubs: LocationSet,
    pub min_connection_minutes: u32,
    pub max_connection_minutes: u32,
}

impl ViaHub {
    pub fn is_stopover(&self) -> bool {
        self.min_connection_minutes >= 20 * 60
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Companion {
    pub name: String,
    pub origin: LocationSet,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupGoal {
    #[default]
    MinTotalCost,
    AlignArrivals {
        max_spread_minutes: u32,
    },
}

/// Extra costs that turn the ticket price into a door-to-door cost.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CostModel {
    /// Round-trip cost of reaching each airport from home.
    pub airport_access: BTreeMap<Iata, Decimal>,
    /// Hotel cost for each layover spanning the night.
    pub overnight_layover: Option<Decimal>,
}

impl CostModel {
    pub fn is_empty(&self) -> bool {
        self.airport_access.is_empty() && self.overnight_layover.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorPolicy {
    pub poll_interval_minutes: u32,
    pub query_budget: u32,
    /// Poll faster close to departure or with volatile prices, slower otherwise.
    pub adaptive: bool,
}

impl Default for MonitorPolicy {
    fn default() -> Self {
        Self {
            poll_interval_minutes: 6 * 60,
            query_budget: 40,
            adaptive: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchDefinition {
    pub name: String,
    pub origin: LocationSet,
    pub destination: LocationSet,
    pub mode: TripMode,
    pub outbound: DateSpec,
    #[serde(default)]
    pub stay: Option<StayLength>,
    /// Open-jaw: where the return flight departs from (defaults to the destination).
    #[serde(default)]
    pub return_origin: Option<LocationSet>,
    /// Open-jaw: where the return flight lands (defaults to the origin).
    #[serde(default)]
    pub return_destination: Option<LocationSet>,
    #[serde(default)]
    pub constraints: ItineraryConstraints,
    pub passengers: u8,
    pub currency: String,
    #[serde(default)]
    pub policy: MonitorPolicy,
    #[serde(default)]
    pub alerts: Vec<AlertRule>,
    #[serde(default)]
    pub via: Option<ViaHub>,
    #[serde(default)]
    pub companions: Vec<Companion>,
    #[serde(default)]
    pub group_goal: GroupGoal,
    #[serde(default)]
    pub costs: CostModel,
}

impl SearchDefinition {
    pub fn return_origin(&self) -> &LocationSet {
        self.return_origin.as_ref().unwrap_or(&self.destination)
    }

    pub fn return_destination(&self) -> &LocationSet {
        self.return_destination.as_ref().unwrap_or(&self.origin)
    }

    pub fn is_open_jaw(&self) -> bool {
        self.return_origin.is_some() || self.return_destination.is_some()
    }

    /// Dates on which a return flight may depart, given the outbound window and stay.
    pub fn return_dates(&self) -> Vec<NaiveDate> {
        let Some(stay) = self.stay else {
            return Vec::new();
        };
        let first = self
            .outbound
            .first()
            .checked_add_days(Days::new(u64::from(stay.min_nights)));
        let last = self
            .outbound
            .last()
            .checked_add_days(Days::new(u64::from(stay.max_nights)));
        match (first, last) {
            (Some(first), Some(last)) => DateSpec::Window {
                from: first,
                to: last,
            }
            .dates(),
            _ => Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        let fail = |msg: &str| Err(DomainError::InvalidSearch(msg.to_string()));
        if self.name.trim().is_empty() {
            return fail("il nome è obbligatorio");
        }
        if !(1..=9).contains(&self.passengers) {
            return fail("i passeggeri devono essere tra 1 e 9");
        }
        if self.currency.len() != 3 || !self.currency.bytes().all(|b| b.is_ascii_uppercase()) {
            return fail("la valuta deve essere un codice ISO di 3 lettere (es. EUR)");
        }
        if self.outbound.first() > self.outbound.last() {
            return fail("la data iniziale è successiva a quella finale");
        }
        let span = (self.outbound.last() - self.outbound.first()).num_days();
        if span > MAX_WINDOW_DAYS as i64 {
            return fail("la finestra di date è troppo ampia (max 330 giorni)");
        }
        if matches!(self.origin, LocationSet::Anywhere)
            || matches!(self.return_destination, Some(LocationSet::Anywhere))
        {
            return fail("'Ovunque' è ammesso solo come destinazione");
        }
        match (self.mode.is_round_trip(), self.stay) {
            (true, None) => {
                return fail("le ricerche andata/ritorno richiedono la durata del soggiorno");
            }
            (true, Some(stay)) if stay.min_nights > stay.max_nights => {
                return fail("le notti minime superano le massime");
            }
            _ => {}
        }
        if matches!(self.destination, LocationSet::Anywhere)
            && (self.mode == TripMode::RoundTripDecoupled
                || self.via.is_some()
                || !self.companions.is_empty())
        {
            return fail("'Ovunque' funziona con solo andata o A/R abbinato, senza hub né gruppo");
        }
        if self.is_open_jaw() && self.mode != TripMode::RoundTripDecoupled {
            return fail("l'open-jaw richiede la modalità A/R libero");
        }
        if self.mode == TripMode::RoundTripCoupled
            && (self.via.is_some() || !self.companions.is_empty())
        {
            return fail("hub e ricerche di gruppo richiedono solo andata o A/R libero");
        }
        if let Some(via) = &self.via
            && via.min_connection_minutes > via.max_connection_minutes
        {
            return fail("la connessione minima all'hub supera la massima");
        }
        if let (Some(min), Some(max)) = (
            self.constraints.min_layover_minutes,
            self.constraints.max_layover_minutes,
        ) && min > max
        {
            return fail("lo scalo minimo supera il massimo");
        }
        if self.companions.iter().any(|c| c.name.trim().is_empty()) {
            return fail("ogni compagno di viaggio deve avere un nome");
        }
        if self.policy.poll_interval_minutes < MIN_POLL_MINUTES {
            return fail("la frequenza minima di polling è 30 minuti");
        }
        if self.policy.query_budget == 0 {
            return fail("il budget di query deve essere almeno 1");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSearch {
    pub id: SearchId,
    pub active: bool,
    pub definition: SearchDefinition,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn definition() -> SearchDefinition {
        SearchDefinition {
            name: "Giappone".into(),
            origin: LocationSet::Airport(Iata::new("FCO").unwrap()),
            destination: LocationSet::Country("JP".into()),
            mode: TripMode::RoundTripDecoupled,
            outbound: DateSpec::Window {
                from: date("2026-12-01"),
                to: date("2026-12-03"),
            },
            stay: Some(StayLength {
                min_nights: 7,
                max_nights: 10,
            }),
            return_origin: None,
            return_destination: None,
            constraints: ItineraryConstraints::default(),
            passengers: 1,
            currency: "EUR".into(),
            policy: MonitorPolicy::default(),
            alerts: vec![AlertRule::NewHistoricMinimum],
            via: None,
            companions: Vec::new(),
            group_goal: GroupGoal::default(),
            costs: CostModel::default(),
        }
    }

    #[test]
    fn window_expands_inclusively() {
        assert_eq!(definition().outbound.dates().len(), 3);
    }

    #[test]
    fn return_dates_cover_every_stay() {
        let dates = definition().return_dates();
        assert_eq!(dates.first(), Some(&date("2026-12-08")));
        assert_eq!(dates.last(), Some(&date("2026-12-13")));
    }

    #[test]
    fn open_jaw_requires_decoupled_mode() {
        let mut def = definition();
        def.return_origin = Some(LocationSet::City("Osaka".into()));
        assert!(def.validate().is_ok());
        def.mode = TripMode::RoundTripCoupled;
        assert!(def.validate().is_err());
    }

    #[test]
    fn round_trip_requires_stay() {
        let mut def = definition();
        def.stay = None;
        assert!(def.validate().is_err());
    }

    #[test]
    fn hour_range_wraps_midnight() {
        let night = HourRange {
            from: NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
            to: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };
        assert!(night.contains(NaiveTime::from_hms_opt(23, 30, 0).unwrap()));
        assert!(night.contains(NaiveTime::from_hms_opt(5, 0, 0).unwrap()));
        assert!(!night.contains(NaiveTime::from_hms_opt(12, 0, 0).unwrap()));
    }

    #[test]
    fn definition_round_trips_through_json() {
        let def = definition();
        let json = serde_json::to_string(&def).unwrap();
        let back: SearchDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(back, def);
        assert!(json.contains("\"kind\":\"country\""));
    }
}
