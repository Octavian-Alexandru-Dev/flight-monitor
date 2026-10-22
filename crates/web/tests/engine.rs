#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use chrono::Utc;
use common::*;
use domain::{AlertRule, Decimal};
use providers::ProviderError;
use storage::HealthStatus;
use web::engine::{Engine, EngineSettings};

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
    let silent = FixtureProvider::silent();
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

#[tokio::test]
async fn country_search_runs_fewer_live_queries_after_a_scan() {
    let mut def = definition(Vec::new());
    def.origin = domain::LocationSet::Airport(iata("FCO"));
    def.destination = domain::LocationSet::Country("JP".into());
    def.outbound = domain::DateSpec::Window {
        from: "2026-12-01".parse().unwrap(),
        to: "2026-12-30".parse().unwrap(),
    };
    def.policy.query_budget = 40;

    let (verify_only, live_alone, _) = setup(chrono::Duration::zero()).await;
    let search = saved(&verify_only, def.clone()).await;
    let report = verify_only.run_searches(vec![search]).await.unwrap();
    assert_eq!(
        live_alone.calls(),
        40,
        "verify-only spends the whole budget"
    );
    assert_eq!(report.executed, 40);

    let live = Arc::new(FixtureProvider::new());
    let scan = Arc::new(ScanProvider::new());
    let with_scan = Engine::new(
        storage::Db::in_memory().await.unwrap(),
        verify_only.geo.clone(),
        live.clone(),
        Some(scan.clone()),
        Arc::new(alerts::RecordingNotifier::default()),
        EngineSettings {
            cache_ttl: chrono::Duration::zero(),
            verify_top_n: 8,
            public_url: None,
        },
    );
    let search = saved(&with_scan, def).await;
    with_scan.run_searches(vec![search]).await.unwrap();
    let japan = with_scan
        .geo
        .resolve(&domain::LocationSet::Country("JP".into()))
        .unwrap()
        .len();
    assert_eq!(
        scan.calls(),
        japan,
        "one cached query per airport pair for December"
    );
    assert_eq!(
        live.calls(),
        8,
        "only the best quoted dates are verified live"
    );
    assert!(live.calls() < live_alone.calls());
}
