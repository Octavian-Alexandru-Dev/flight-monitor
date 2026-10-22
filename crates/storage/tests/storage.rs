#![allow(clippy::unwrap_used)]

use chrono::{Duration, Utc};
use domain::{
    AlertRule, CostModel, DateSpec, Decimal, Direction, GroupGoal, Iata, Itinerary,
    ItineraryConstraints, LocationSet, MonitorPolicy, ProviderId, SearchDefinition, Segment,
    Ticket, TripMode, TripOption,
};
use storage::{Db, HealthStatus, NewAlert, NewQueryRun, NewSearchRun, ProviderHealth};

fn definition(name: &str) -> SearchDefinition {
    SearchDefinition {
        name: name.into(),
        origin: LocationSet::Airport(Iata::new("FCO").unwrap()),
        destination: LocationSet::Airport(Iata::new("NRT").unwrap()),
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
        policy: MonitorPolicy::default(),
        alerts: vec![AlertRule::NewHistoricMinimum],
        via: None,
        companions: Vec::new(),
        group_goal: GroupGoal::default(),
        costs: CostModel::default(),
    }
}

fn itinerary(price: i64) -> Itinerary {
    Itinerary::new(
        vec![Segment {
            from: Iata::new("FCO").unwrap(),
            to: Iata::new("NRT").unwrap(),
            departure: "2026-12-10T10:00:00".parse().unwrap(),
            arrival: "2026-12-11T08:00:00".parse().unwrap(),
            duration_minutes: 840,
            airline: "AZ".into(),
            airline_name: "ITA".into(),
            flight_number: "784".into(),
        }],
        Decimal::new(price, 0),
        "EUR".into(),
        ProviderId::new("test"),
        Utc::now(),
    )
    .unwrap()
}

#[tokio::test]
async fn migrations_apply_and_searches_round_trip() {
    let db = Db::in_memory().await.unwrap();
    let id = db.create_search(&definition("Tokyo")).await.unwrap();

    let stored = db.get_search(id).await.unwrap();
    assert_eq!(stored.search.definition, definition("Tokyo"));
    assert!(stored.search.active);

    db.update_search(id, &definition("Tokyo 2")).await.unwrap();
    assert_eq!(
        db.list_searches().await.unwrap()[0].search.definition.name,
        "Tokyo 2"
    );

    let now = Utc::now();
    assert_eq!(db.due_searches(now).await.unwrap().len(), 1);
    db.mark_search_ran(id, now, now + Duration::hours(6))
        .await
        .unwrap();
    assert!(db.due_searches(now).await.unwrap().is_empty());

    db.set_search_active(id, false).await.unwrap();
    assert!(
        db.due_searches(now + Duration::days(1))
            .await
            .unwrap()
            .is_empty()
    );

    db.delete_search(id).await.unwrap();
    assert!(db.get_search(id).await.is_err());
}

#[tokio::test]
async fn query_runs_feed_cache_and_history() {
    let db = Db::in_memory().await.unwrap();
    let provider = ProviderId::new("google_flights");
    let started = Utc::now();
    let itineraries = [itinerary(500), itinerary(450)];
    db.record_query_run(NewQueryRun {
        provider: &provider,
        query_key: "k1",
        description: "FCO→NRT",
        started_at: started,
        duration_ms: 1200,
        outcome: "ok",
        error: None,
        itineraries: &itineraries,
    })
    .await
    .unwrap();

    let cached = db
        .cached_itineraries("k1", started - Duration::minutes(5))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cached, itineraries);
    assert!(
        db.cached_itineraries("k1", started + Duration::minutes(5))
            .await
            .unwrap()
            .is_none()
    );

    let history = db.query_history().await.unwrap();
    assert_eq!(history["k1"].best_price, Some(Decimal::new(450, 0)));
    assert_eq!(history["k1"].runs_with_results, 1);

    let stats = db
        .route_stats(
            "test",
            &Iata::new("FCO").unwrap(),
            &Iata::new("NRT").unwrap(),
            "EUR",
        )
        .await
        .unwrap();
    assert_eq!(stats.observations, 2);
    assert_eq!(stats.mean, Decimal::new(475, 0));
    assert_eq!(stats.std_dev, Decimal::new(25, 0));
}

#[tokio::test]
async fn search_runs_track_history_and_alerts_deduplicate() {
    let db = Db::in_memory().await.unwrap();
    let id = db.create_search(&definition("Tokyo")).await.unwrap();
    let option = |price| {
        TripOption::new(
            vec![Ticket {
                traveler: None,
                direction: Direction::Outbound,
                itinerary: itinerary(price),
            }],
            "EUR".into(),
        )
    };
    let mut runs = Vec::new();
    for price in [600, 520] {
        let options = [option(price)];
        let now = Utc::now();
        runs.push(
            db.record_search_run(NewSearchRun {
                search_id: id,
                started_at: now,
                finished_at: now,
                queries_planned: 1,
                queries_executed: 1,
                queries_failed: 0,
                currency: "EUR",
                options: &options,
                calendar: &[],
            })
            .await
            .unwrap(),
        );
    }
    assert_eq!(
        db.previous_best(id, runs[1]).await.unwrap(),
        Some(Decimal::new(600, 0))
    );
    assert_eq!(
        db.historic_min(id, runs[1]).await.unwrap(),
        Some(Decimal::new(600, 0))
    );
    let latest = db.latest_search_run(id).await.unwrap().unwrap();
    assert_eq!(latest.best_price, Some(Decimal::new(520, 0)));
    assert_eq!(db.run_points(id, 10).await.unwrap().len(), 2);

    let key = option(520).key();
    assert!(
        !db.alert_already_sent(id, &key, Decimal::new(520, 0))
            .await
            .unwrap()
    );
    db.record_alert(NewAlert {
        search_id: id,
        option_key: &key,
        price: Decimal::new(520, 0),
        reasons: "drop",
        message: "msg",
    })
    .await
    .unwrap();
    assert!(
        db.alert_already_sent(id, &key, Decimal::new(520, 0))
            .await
            .unwrap()
    );
    assert!(
        !db.alert_already_sent(id, &key, Decimal::new(500, 0))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn provider_health_upserts() {
    let db = Db::in_memory().await.unwrap();
    let mut health = db.provider_health("google_flights").await.unwrap();
    assert_eq!(health.status, HealthStatus::Ok);
    health.status = HealthStatus::Degraded;
    health.consecutive_failures = 3;
    db.save_provider_health(&health).await.unwrap();
    db.save_provider_health(&health).await.unwrap();
    let all = db.all_provider_health().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, HealthStatus::Degraded);
    assert_eq!(ProviderHealth::new("x").consecutive_failures, 0);

    db.set_state("daily", "2026-10-22").await.unwrap();
    assert_eq!(
        db.state("daily").await.unwrap().as_deref(),
        Some("2026-10-22")
    );
}
