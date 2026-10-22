#![allow(clippy::unwrap_used)]

mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use common::*;
use tower::ServiceExt;
use web::server::{AppState, FormDefaults, router};

async fn app() -> (axum::Router, std::sync::Arc<web::engine::Engine>) {
    let (engine, _, _) = setup(chrono::Duration::minutes(60)).await;
    let engine = std::sync::Arc::new(engine);
    let state = AppState::new(
        engine.clone(),
        FormDefaults {
            budget: 20,
            poll_hours: 6,
        },
        None,
    );
    (router(state), engine)
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn post(app: &axum::Router, uri: &str, form: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::post(uri)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

const FORM: &str = "name=Tokyo+test&origin_kind=airports&origin_value=FCO%2C+MXP&dest_kind=city&dest_value=Tokyo\
&mode=one_way&date_from=2026-12-10&date_to=&max_stops=1&checked_bags=0&passengers=1&currency=EUR\
&poll_hours=6&budget=10&adaptive=on&alert_new_min=on&group_goal=min_cost";

#[tokio::test]
async fn empty_dashboard_invites_to_create_a_search() {
    let (app, _) = app().await;
    let (status, body) = get(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Nessuna ricerca salvata"));
    assert!(!body.contains('—'), "no em-dashes in the UI");
}

#[tokio::test]
async fn whole_flow_from_the_browser() {
    let (app, engine) = app().await;

    let (status, body) = get(&app, "/searches/new").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Stima per ciclo"));

    let estimate = post(&app, "/searches/estimate", FORM).await;
    let estimate =
        String::from_utf8_lossy(&to_bytes(estimate.into_body(), usize::MAX).await.unwrap())
            .into_owned();
    assert!(estimate.contains("query live per ciclo"), "{estimate}");

    let created = post(&app, "/searches", FORM).await;
    assert_eq!(created.status(), StatusCode::SEE_OTHER);
    let location = created.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(location, "/searches/1");

    let search = engine
        .db
        .get_search(domain::SearchId(1))
        .await
        .unwrap()
        .search;
    engine.run_searches(vec![search]).await.unwrap();

    let (status, detail) = get(&app, &location).await;
    assert_eq!(status, StatusCode::OK);
    assert!(detail.contains("Prezzo migliore"));
    assert!(detail.contains("Migliori opzioni"));
    assert!(
        detail.contains("google.com/travel/flights"),
        "buy links are shown"
    );
    assert!(detail.contains("Calendario prezzi"));

    let (_, dashboard) = get(&app, "/").await;
    assert!(dashboard.contains("Tokyo test"));

    let (status, edit) = get(&app, "/searches/1/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(edit.contains("FCO, MXP"));

    let updated = post(
        &app,
        "/searches/1",
        &FORM.replace("Tokyo+test", "Tokyo+rinominata"),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::SEE_OTHER);
    let toggled = post(&app, "/searches/1/toggle", "").await;
    assert_eq!(toggled.status(), StatusCode::SEE_OTHER);
    let (_, dashboard) = get(&app, "/").await;
    assert!(dashboard.contains("Tokyo rinominata"));
    assert!(dashboard.contains("In pausa"));

    let (status, providers) = get(&app, "/providers").await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.contains("FCO,MXP"));

    let deleted = post(&app, "/searches/1/delete", "").await;
    assert_eq!(deleted.status(), StatusCode::SEE_OTHER);
    assert_eq!(get(&app, "/searches/1").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_form_is_rejected_with_field_errors() {
    let (app, _) = app().await;
    let response = post(&app, "/searches", &FORM.replace("FCO%2C+MXP", "ZZZ")).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = String::from_utf8_lossy(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
        .into_owned();
    assert!(body.contains("Aeroporto sconosciuto: ZZZ"), "{body}");
    assert!(body.contains("aria-invalid=\"true\""));
}

#[tokio::test]
async fn location_preview_lists_resolved_airports() {
    let (app, _) = app().await;
    let (status, body) = get(
        &app,
        "/geo/preview?prefix=origin&origin_kind=city&origin_value=Milano",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for code in ["MXP", "LIN", "BGY"] {
        assert!(body.contains(code), "{code} missing");
    }
    assert!(body.contains("suggest-origin"));
}

#[tokio::test]
async fn static_assets_are_embedded() {
    let (app, _) = app().await;
    for asset in [
        "app.css",
        "app.js",
        "htmx.min.js",
        "icons.css",
        "icons.woff2",
        "Geist-Variable.woff2",
    ] {
        assert_eq!(
            get(&app, &format!("/static/{asset}")).await.0,
            StatusCode::OK,
            "{asset}"
        );
    }
    assert_eq!(
        get(&app, "/static/missing.js").await.0,
        StatusCode::NOT_FOUND
    );
}
