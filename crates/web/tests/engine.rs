#![allow(clippy::unwrap_used)]

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
use storage::{Db, HealthStatus};
use web::engine::{Engine, EngineSettings};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn iata(code: &str) -> Iata {
    Iata::new(code).unwrap()
}

/// Serves the recorded Google Flights answer, with prices scaled on demand.
struct FixtureProvider {
    itineraries: Vec<Itinerary>,
    factor: Mutex<Decimal>,
    fail_with: Mutex<Option<ProviderError>>,
    calls: AtomicUsize,
    capabilities: Capabilities,
}

impl FixtureProvider {
    fn new() -> Self {
        let raw = std::fs::read(root().join("fixtures/google_flights/one_way_multi_airport.json"))
            .unwrap();
        let envelope: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        let reply: GoogleReply = serde_json::from_value(envelope["result"].clone()).unwrap();
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

    fn scale(&self, factor: Decimal) {
        *self.factor.lock().unwrap() = factor;
    }

    fn fail(&self, error: Option<ProviderError>) {
        *self.fail_with.lock().unwrap() = error;
    }

    fn calls(&self) -> usize {
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

fn definition(alerts: Vec<AlertRule>) -> SearchDefinition {
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

async fn setup(
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

async fn saved(engine: &Engine, def: SearchDefinition) -> SavedSearch {
    let id = engine.db.create_search(&def).await.unwrap();
    engine.db.get_search(id).await.unwrap().search
}

#[tokio::test]
async fn saved_search_runs_periodically_and_fills_history() {
    let (engine, provider, _) = setup(chrono::Duration::minutes(60)).await;
    let search = saved(&engine, definition(Vec::new())).await;

    let report = engine.run_due().await.unwrap();
    assert_eq!((report.searches, report.executed), (1, 1));
    let run = engine
        .db
        .latest_search_run(search.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!run.options.is_empty());
    assert_eq!(
        run.best_price,
        run.options.iter().map(|o| o.total_price).min()
    );
    assert_eq!(
        engine.db.recent_query_runs(10).await.unwrap()[0].result_count,
        run.options.len() as i64
    );

    let record = engine.db.get_search(search.id).await.unwrap();
    let next = record.next_run_at.unwrap();
    assert!((next - Utc::now()).num_minutes() >= 359);
    assert_eq!(engine.run_due().await.unwrap().searches, 0, "not due yet");

    let report = engine.run_searches(vec![search.clone()]).await.unwrap();
    assert_eq!(
        (report.executed, report.cached),
        (0, 1),
        "served from cache"
    );
    assert_eq!(provider.calls(), 1);
    assert_eq!(engine.db.run_points(search.id, 10).await.unwrap().len(), 2);
}

#[tokio::test]
async fn identical_queries_from_two_searches_run_once() {
    let (engine, provider, _) = setup(chrono::Duration::zero()).await;
    let a = saved(&engine, definition(Vec::new())).await;
    let b = saved(&engine, definition(vec![AlertRule::NewHistoricMinimum])).await;
    let report = engine
        .run_searches(vec![a.clone(), b.clone()])
        .await
        .unwrap();
    assert_eq!(report.executed, 1);
    assert_eq!(provider.calls(), 1);
    for id in [a.id, b.id] {
        assert!(
            engine
                .db
                .latest_search_run(id)
                .await
                .unwrap()
                .unwrap()
                .best_price
                .is_some()
        );
    }
}

#[tokio::test]
async fn simulated_price_drop_sends_exactly_one_message() {
    let (engine, provider, notifier) = setup(chrono::Duration::zero()).await;
    let search = saved(
        &engine,
        definition(vec![
            AlertRule::DropPercent(Decimal::from(10)),
            AlertRule::NewHistoricMinimum,
        ]),
    )
    .await;

    engine.run_searches(vec![search.clone()]).await.unwrap();
    assert!(
        notifier.messages().is_empty(),
        "first run has nothing to compare with"
    );

    provider.scale(Decimal::new(8, 1));
    let report = engine.run_searches(vec![search.clone()]).await.unwrap();
    assert_eq!(report.alerts, 1);
    engine.run_searches(vec![search.clone()]).await.unwrap();

    let messages = notifier.messages();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("Tokyo a dicembre"));
    assert!(messages[0].contains("Calo del 19.9%"), "{}", messages[0]);
    assert!(messages[0].contains("Nuovo minimo storico"));
    assert!(messages[0].contains("http://localhost:8080/searches/"));
}

#[tokio::test]
async fn below_threshold_alert_is_not_repeated() {
    let (engine, _, notifier) = setup(chrono::Duration::zero()).await;
    let search = saved(
        &engine,
        definition(vec![AlertRule::BelowPrice(Decimal::from(5000))]),
    )
    .await;
    for _ in 0..3 {
        engine.run_searches(vec![search.clone()]).await.unwrap();
    }
    assert_eq!(notifier.messages().len(), 1);
}

#[tokio::test]
async fn failing_source_is_degraded_reported_and_recovered() {
    let (engine, provider, notifier) = setup(chrono::Duration::zero()).await;
    let search = saved(&engine, definition(Vec::new())).await;
    provider.fail(Some(ProviderError::Parse("layout changed".into())));
    for _ in 0..3 {
        let report = engine.run_searches(vec![search.clone()]).await.unwrap();
        assert_eq!(report.failed, 1);
    }
    let health = engine.db.provider_health("fixture").await.unwrap();
    assert_eq!(health.status, HealthStatus::Degraded);
    let run = engine
        .db
        .latest_search_run(search.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run.queries_failed, 1,
        "a failure is recorded, not mistaken for no flights"
    );

    provider.fail(None);
    engine.run_searches(vec![search]).await.unwrap();
    assert_eq!(
        engine.db.provider_health("fixture").await.unwrap().status,
        HealthStatus::Ok
    );
    let messages = notifier.messages();
    assert_eq!(messages.len(), 2);
    assert!(messages[0].contains("Fonte degradata"));
    assert!(messages[1].contains("Fonte ripristinata"));
}

#[tokio::test]
async fn silent_empty_answers_degrade_the_source() {
    let (engine, _, notifier) = setup(chrono::Duration::zero()).await;
    let search = saved(&engine, definition(Vec::new())).await;
    engine.run_searches(vec![search.clone()]).await.unwrap();
    engine.run_searches(vec![search.clone()]).await.unwrap();
    // Same source and route, now silently answering nothing.
    let silent = FixtureProvider {
        itineraries: Vec::new(),
        ..FixtureProvider::new()
    };
    let engine = Engine::new(
        engine.db.clone(),
        engine.geo.clone(),
        Arc::new(silent),
        None,
        notifier.clone(),
        EngineSettings {
            cache_ttl: chrono::Duration::zero(),
            verify_top_n: 8,
            public_url: None,
        },
    );
    for _ in 0..3 {
        engine.run_searches(vec![search.clone()]).await.unwrap();
    }
    assert_eq!(
        engine.db.provider_health("fixture").await.unwrap().status,
        HealthStatus::Degraded
    );
    assert!(
        notifier
            .messages()
            .iter()
            .any(|m| m.contains("di solito ne ha"))
    );
}
