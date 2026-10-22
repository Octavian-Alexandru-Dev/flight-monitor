#![allow(clippy::unwrap_used, dead_code)]

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alerts::RecordingNotifier;
use async_trait::async_trait;
use chrono::Utc;
use domain::{
    AlertRule, Capabilities, CostModel, DateSpec, Decimal, GroupGoal, Iata, Itinerary,
    ItineraryConstraints, LocationSet, MonitorPolicy, ProviderId, ProviderQuery, QueryLeg,
    SavedSearch, SearchDefinition, TripMode,
};
use geo::GeoIndex;
use providers::{FlightProvider, GoogleReply, ProviderError};
use storage::Db;
use web::engine::{Engine, EngineSettings};

pub fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

pub fn iata(code: &str) -> Iata {
    Iata::new(code).unwrap()
}

/// Serves the recorded Google Flights answer, with prices scaled on demand.
pub struct FixtureProvider {
    itineraries: Vec<Itinerary>,
    factor: Mutex<Decimal>,
    fail_with: Mutex<Option<ProviderError>>,
    calls: AtomicUsize,
    capabilities: Capabilities,
}

impl FixtureProvider {
    pub fn new() -> Self {
        let raw = std::fs::read(root().join("fixtures/google_flights/one_way_multi_airport.json"))
            .unwrap();
        let reply: GoogleReply = serde_json::from_slice(&raw).unwrap();
        let query = ProviderQuery {
            legs: vec![QueryLeg::on(
                vec![iata("FCO"), iata("MXP")],
                vec![iata("NRT"), iata("HND")],
                "2026-12-10".parse().unwrap(),
            )],
            max_stops: 1,
            passengers: 1,
            currency: "EUR".into(),
            checked_bags: 0,
        };
        Self {
            itineraries: reply.itineraries(&query, Utc::now()).unwrap(),
            factor: Mutex::new(Decimal::ONE),
            fail_with: Mutex::new(None),
            calls: AtomicUsize::new(0),
            capabilities: Capabilities {
                multi_airport_query: true,
                max_airports_per_side: 7,
                city_query: false,
                max_stops_filter: true,
                round_trip: true,
                date_range: false,
                open_destination: false,
                live: true,
                min_interval: Duration::ZERO,
            },
        }
    }

    /// A source that answers every query with nothing.
    pub fn silent() -> Self {
        Self {
            itineraries: Vec::new(),
            ..Self::new()
        }
    }

    pub fn scale(&self, factor: Decimal) {
        *self.factor.lock().unwrap() = factor;
    }

    pub fn fail(&self, error: Option<ProviderError>) {
        *self.fail_with.lock().unwrap() = error;
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl FlightProvider for FixtureProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("fixture")
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn search(&self, query: &ProviderQuery) -> Result<Vec<Itinerary>, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.fail_with.lock().unwrap().clone() {
            return Err(error);
        }
        let factor = *self.factor.lock().unwrap();
        let leg = &query.legs[0];
        Ok(self
            .itineraries
            .iter()
            .filter(|i| leg.covers(i.origin(), i.destination(), i.departure_date()))
            .map(|i| {
                let mut scaled = i.clone();
                scaled.price = (i.price * factor).round();
                scaled.fetched_at = Utc::now();
                scaled
            })
            .collect())
    }
}

pub fn definition(alerts: Vec<AlertRule>) -> SearchDefinition {
    SearchDefinition {
        name: "Tokyo a dicembre".into(),
        origin: LocationSet::Airports(vec![iata("FCO"), iata("MXP")]),
        destination: LocationSet::City("Tokyo".into()),
        mode: TripMode::OneWay,
        outbound: DateSpec::Fixed {
            date: "2026-12-10".parse().unwrap(),
        },
        stay: None,
        return_origin: None,
        return_destination: None,
        constraints: ItineraryConstraints::default(),
        passengers: 1,
        currency: "EUR".into(),
        policy: MonitorPolicy {
            poll_interval_minutes: 360,
            query_budget: 10,
            adaptive: false,
        },
        alerts,
        via: None,
        companions: Vec::new(),
        group_goal: GroupGoal::default(),
        costs: CostModel::default(),
    }
}

pub async fn setup(
    cache_ttl: chrono::Duration,
) -> (Engine, Arc<FixtureProvider>, Arc<RecordingNotifier>) {
    let provider = Arc::new(FixtureProvider::new());
    let notifier = Arc::new(RecordingNotifier::default());
    let engine = Engine::new(
        Db::in_memory().await.unwrap(),
        Arc::new(GeoIndex::load(&root().join("data")).unwrap()),
        provider.clone(),
        None,
        notifier.clone(),
        EngineSettings {
            cache_ttl,
            verify_top_n: 8,
            public_url: Some("http://localhost:8080".into()),
        },
    );
    (engine, provider, notifier)
}

pub async fn saved(engine: &Engine, def: SearchDefinition) -> SavedSearch {
    let id = engine.db.create_search(&def).await.unwrap();
    engine.db.get_search(id).await.unwrap().search
}

/// A cached-price source with one FCO→NRT quote per December day, cheaper on the 15th.
pub struct ScanProvider {
    quotes: Vec<Itinerary>,
    calls: AtomicUsize,
    capabilities: Capabilities,
}

impl ScanProvider {
    pub fn new() -> Self {
        let quotes = (1..=30)
            .map(|day| {
                let departure = format!("2026-12-{day:02}T10:00:00").parse().unwrap();
                Itinerary::new(
                    vec![domain::Segment {
                        from: iata("FCO"),
                        to: iata("NRT"),
                        departure,
                        arrival: departure + chrono::Duration::hours(16),
                        duration_minutes: 960,
                        airline: "XX".into(),
                        airline_name: "Cache".into(),
                        flight_number: "1".into(),
                    }],
                    Decimal::from(if day == 15 { 380 } else { 400 + day }),
                    "EUR".into(),
                    ProviderId::new("scan"),
                    Utc::now(),
                )
                .unwrap()
            })
            .collect();
        Self {
            quotes,
            calls: AtomicUsize::new(0),
            capabilities: Capabilities {
                multi_airport_query: false,
                max_airports_per_side: 1,
                city_query: true,
                max_stops_filter: false,
                round_trip: false,
                date_range: true,
                open_destination: true,
                live: false,
                min_interval: Duration::ZERO,
            },
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl FlightProvider for ScanProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("scan")
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn search(&self, query: &ProviderQuery) -> Result<Vec<Itinerary>, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let leg = &query.legs[0];
        Ok(self
            .quotes
            .iter()
            .filter(|q| leg.covers(q.origin(), q.destination(), q.departure_date()))
            .cloned()
            .collect())
    }
}
