use std::collections::HashMap;

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::{Local, Utc};
use domain::{Decimal, Iata, SearchId};
use geo::{GeoIndex, SuggestionKind};
use planner::Strategy;
use serde::Deserialize;
use storage::{HealthStatus, RunPoint};

use super::charts::{heatmap, history_chart, sparkline};
use super::form::{FormErrors, SearchForm, parse_location};
use super::views::{
    AdviceView, Delta, OptionView, alerts_summary, dates, delta, local_time, mode_label, money,
    option_view, relative, route, settings_summary,
};
use super::{AppError, AppResult, AppState, render};

const SPARK_POINTS: i64 = 24;
const HISTORY_POINTS: i64 = 500;
const OPTIONS_SHOWN: usize = 20;

// ---------- dashboard ----------

struct SearchRow {
    id: i64,
    name: String,
    route: String,
    mode: &'static str,
    dates: String,
    active: bool,
    running: bool,
    best: Option<String>,
    delta: Option<Delta>,
    spark: Option<String>,
    schedule: String,
    trouble: Option<String>,
}

struct HealthView {
    name: String,
    ok: bool,
    detail: String,
}

struct AlertView {
    search: String,
    search_id: i64,
    price: String,
    reasons: String,
    when: String,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardPage {
    nav: &'static str,
    rows: Vec<SearchRow>,
    health: Vec<HealthView>,
    degraded: Vec<String>,
    alerts: Vec<AlertView>,
    telegram: bool,
}

fn reason_label(code: &str) -> &'static str {
    match code {
        "below_price" => "sotto soglia",
        "drop" => "calo",
        "new_minimum" => "nuovo minimo",
        "anomaly" => "anomalia",
        _ => "alert",
    }
}

fn trouble(points: &[RunPoint]) -> Option<String> {
    let last = points.last()?;
    match (last.queries_failed, last.best_price) {
        (0, _) => None,
        (failed, None) if failed == last.queries_planned => {
            Some("Fonte non raggiungibile nell'ultimo controllo".into())
        }
        (failed, _) => Some(format!("{failed} query fallite nell'ultimo controllo")),
    }
}

pub async fn dashboard(State(state): State<AppState>) -> AppResult<Html<String>> {
    let db = &state.engine.db;
    let geo = &state.engine.geo;
    let now = Utc::now();
    let mut rows = Vec::new();
    let mut names = HashMap::new();
    for record in db.list_searches().await? {
        let search = &record.search;
        let def = &search.definition;
        names.insert(search.id, (def.name.clone(), def.currency.clone()));
        let points = db.run_points(search.id, SPARK_POINTS).await?;
        let prices: Vec<Decimal> = points.iter().filter_map(|p| p.best_price).collect();
        let current = points.last().and_then(|p| p.best_price);
        let previous = points.iter().rev().skip(1).find_map(|p| p.best_price);
        let running = state.is_running(search.id);
        let schedule = if running {
            "In esecuzione".to_string()
        } else if !search.active {
            "In pausa".to_string()
        } else {
            match record.next_run_at {
                Some(next) if next > now => format!("Prossimo controllo {}", relative(next, now)),
                _ => "Controllo imminente".to_string(),
            }
        };
        rows.push(SearchRow {
            id: search.id.0,
            name: def.name.clone(),
            route: route(def, geo),
            mode: mode_label(def.mode),
            dates: dates(def),
            active: search.active,
            running,
            best: current.map(|p| money(p, &def.currency)),
            delta: delta(current, previous, &def.currency),
            spark: (prices.len() > 1).then(|| sparkline(&prices)).flatten(),
            schedule,
            trouble: trouble(&points),
        });
    }
    let health = provider_health(&state).await?;
    let degraded = health
        .iter()
        .filter(|h| !h.ok)
        .map(|h| h.name.clone())
        .collect();
    let alerts = db
        .recent_alerts(6)
        .await?
        .into_iter()
        .map(|a| {
            let (search, currency) = names
                .get(&a.search_id)
                .cloned()
                .unwrap_or_else(|| ("Ricerca eliminata".into(), "EUR".into()));
            AlertView {
                search,
                search_id: a.search_id.0,
                price: money(a.price, &currency),
                reasons: a
                    .reasons
                    .split(',')
                    .map(reason_label)
                    .collect::<Vec<_>>()
                    .join(", "),
                when: relative(a.sent_at, now),
            }
        })
        .collect();
    render(&DashboardPage {
        nav: "searches",
        rows,
        health,
        degraded,
        alerts,
        telegram: state.engine.notifier().is_enabled(),
    })
}

async fn provider_health(state: &AppState) -> AppResult<Vec<HealthView>> {
    let mut out = Vec::new();
    for provider in state.engine.providers() {
        let id = provider.id();
        let health = state.engine.db.provider_health(id.as_str()).await?;
        let caps = provider.capabilities();
        let kind = if caps.live { "live" } else { "cache" };
        let detail = match (&health.status, &health.last_error) {
            (HealthStatus::Degraded, Some(error)) => error.clone(),
            _ => match health.last_success_at {
                Some(at) => format!("{kind} · ok {}", relative(at, Utc::now())),
                None => format!("{kind} · nessuna richiesta ancora"),
            },
        };
        out.push(HealthView {
            name: provider_label(id.as_str()).to_string(),
            ok: health.status == HealthStatus::Ok,
            detail,
        });
    }
    Ok(out)
}

fn provider_label(id: &str) -> &str {
    match id {
        "google_flights" => "Google Flights",
        "travelpayouts" => "Travelpayouts",
        other => other,
    }
}

// ---------- detail ----------

#[derive(Template)]
#[template(path = "detail.html")]
struct DetailPage {
    nav: &'static str,
    id: i64,
    name: String,
    route: String,
    mode: &'static str,
    dates: String,
    active: bool,
    running: bool,
    has_run: bool,
    best: Option<String>,
    door_to_door: Option<String>,
    delta: Option<Delta>,
    historic_min: Option<String>,
    advice: Option<AdviceView>,
    last_run: Option<String>,
    next_run: String,
    queries: String,
    source_down: bool,
    partial_failure: Option<String>,
    chart: Option<String>,
    history: Vec<(String, String)>,
    calendar: Option<String>,
    options: Vec<OptionView>,
    more_options: usize,
    settings: Vec<String>,
    alert_rules: Vec<String>,
}

pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Html<String>> {
    let id = SearchId(id);
    let db = &state.engine.db;
    let record = db.get_search(id).await?;
    let def = &record.search.definition;
    let currency = def.currency.as_str();
    let run = db.latest_search_run(id).await?;
    let points = db.run_points(id, HISTORY_POINTS).await?;
    let priced: Vec<_> = points
        .iter()
        .filter_map(|p| p.best_price.map(|price| (p.at, price)))
        .collect();
    let prices: Vec<Decimal> = priced.iter().map(|(_, p)| *p).collect();
    let best = run.as_ref().and_then(|r| r.best_price);
    let previous = match &run {
        Some(r) => db.previous_best(id, r.id).await?,
        None => None,
    };
    let days = (def.outbound.first() - Local::now().date_naive()).num_days();
    let now = Utc::now();
    let running = state.is_running(id);
    let options = run
        .as_ref()
        .map(|r| r.options.as_slice())
        .unwrap_or_default();
    let door_to_door = options
        .first()
        .filter(|o| !o.extra_cost.is_zero())
        .map(|o| money(o.door_to_door(), currency));
    let (source_down, partial_failure) = match &run {
        Some(r) if r.queries_failed > 0 && r.options.is_empty() => (true, None),
        Some(r) if r.queries_failed > 0 => (
            false,
            Some(format!(
                "{} query su {} non sono andate a buon fine: i risultati potrebbero essere incompleti.",
                r.queries_failed, r.queries_planned
            )),
        ),
        _ => (false, None),
    };
    let page = DetailPage {
        nav: "searches",
        id: id.0,
        name: def.name.clone(),
        route: route(def, &state.engine.geo),
        mode: mode_label(def.mode),
        dates: dates(def),
        active: record.search.active,
        running,
        has_run: run.is_some(),
        best: best.map(|p| money(p, currency)),
        door_to_door,
        delta: delta(best, previous, currency),
        historic_min: prices.iter().min().map(|p| money(*p, currency)),
        advice: best.map(|b| planner::advise(&prices, b, days).into()),
        last_run: run.as_ref().map(|r| relative(r.finished_at, now)),
        next_run: match (record.search.active, record.next_run_at) {
            (false, _) => "In pausa".into(),
            (true, Some(next)) if next > now => relative(next, now),
            _ => "a breve".into(),
        },
        queries: run.as_ref().map_or_else(String::new, |r| {
            format!(
                "{} query live · {} fallite",
                r.queries_planned, r.queries_failed
            )
        }),
        source_down,
        partial_failure,
        chart: history_chart(&priced, currency),
        history: priced
            .iter()
            .rev()
            .take(30)
            .map(|(at, p)| (local_time(*at), money(*p, currency)))
            .collect(),
        calendar: run.as_ref().and_then(|r| heatmap(&r.calendar, currency)),
        options: options
            .iter()
            .take(OPTIONS_SHOWN)
            .map(option_view)
            .collect(),
        more_options: options.len().saturating_sub(OPTIONS_SHOWN),
        settings: settings_summary(def, &state.engine.geo),
        alert_rules: alerts_summary(def),
    };
    render(&page)
}

// ---------- form ----------

#[derive(Template)]
#[template(path = "form.html")]
struct FormPage {
    nav: &'static str,
    title: String,
    action: String,
    submit: &'static str,
    form: SearchForm,
    errors: FormErrors,
    estimate: String,
    llm: bool,
    notice: Option<String>,
    cancel: String,
}

struct LocationField {
    prefix: &'static str,
    label: &'static str,
    kind: String,
    value: String,
    error: Option<String>,
    kinds: Vec<(&'static str, &'static str)>,
    help: &'static str,
}

const PLACE_KINDS: [(&str, &str); 4] = [
    ("airports", "Aeroporti"),
    ("city", "Città"),
    ("country", "Paese"),
    ("radius", "Raggio"),
];

impl FormPage {
    fn error(&self, field: &str) -> Option<&String> {
        self.errors.fields.get(field)
    }

    fn checked(&self, field: &str) -> &'static str {
        let value = match field {
            "adaptive" => &self.form.adaptive,
            "alert_new_min" => &self.form.alert_new_min,
            "alert_anomaly" => &self.form.alert_anomaly,
            _ => return "",
        };
        if value.is_some() { "checked" } else { "" }
    }

    fn selected(&self, field: &str, value: &str) -> &'static str {
        let current = match field {
            "group_goal" => &self.form.group_goal,
            _ => return "",
        };
        if current == value { "selected" } else { "" }
    }

    fn advanced_open(&self) -> bool {
        let f = &self.form;
        let filled = [
            &f.return_origin_value,
            &f.return_dest_value,
            &f.via_value,
            &f.companions,
            &f.access_costs,
            &f.overnight_cost,
        ]
        .iter()
        .any(|v| !v.trim().is_empty());
        let failing = [
            "return_origin_value",
            "return_dest_value",
            "via_value",
            "via_min_h",
            "via_max_h",
            "companions",
            "access_costs",
            "overnight_cost",
        ]
        .iter()
        .any(|k| self.errors.fields.contains_key(k));
        filled || failing
    }

    fn location(&self, prefix: &'static str) -> LocationField {
        let f = &self.form;
        let (label, kind, value, extra, help): (_, _, _, Option<(&'static str, &'static str)>, _) =
            match prefix {
                "origin" => (
                    "Partenza",
                    &f.origin_kind,
                    &f.origin_value,
                    None,
                    "Uno o più aeroporti, una città con tutti i suoi scali, un paese o un raggio in km.",
                ),
                "dest" => (
                    "Destinazione",
                    &f.dest_kind,
                    &f.dest_value,
                    Some(("anywhere", "Ovunque")),
                    "«Ovunque» cerca le mete più economiche e richiede il token Travelpayouts.",
                ),
                "return_origin" => (
                    "Ritorno da",
                    &f.return_origin_kind,
                    &f.return_origin_value,
                    Some(("same", "Come la destinazione")),
                    "Lascia «Come la destinazione» per un normale andata e ritorno.",
                ),
                "return_dest" => (
                    "Ritorno verso",
                    &f.return_dest_kind,
                    &f.return_dest_value,
                    Some(("same", "Come la partenza")),
                    "Lascia «Come la partenza» per rientrare dove sei partito.",
                ),
                _ => (
                    "Hub",
                    &f.via_kind,
                    &f.via_value,
                    Some(("", "Nessun hub")),
                    "Città o aeroporti dove cambiare biglietto, ad esempio Milano o IST.",
                ),
            };
        let mut kinds = Vec::new();
        if let Some(first) = extra.filter(|(k, _)| *k != "anywhere") {
            kinds.push(first);
        }
        kinds.extend(PLACE_KINDS);
        if let Some(last) = extra.filter(|(k, _)| *k == "anywhere") {
            kinds.push(last);
        }
        LocationField {
            prefix,
            label,
            kind: kind.clone(),
            value: value.clone(),
            error: self
                .errors
                .fields
                .get(format!("{prefix}_value").as_str())
                .cloned(),
            kinds,
            help,
        }
    }
}

#[derive(Template)]
#[template(path = "partials/estimate.html")]
struct EstimatePartial {
    ok: bool,
    message: String,
    live: usize,
    candidates: usize,
    scan: usize,
    per_day: usize,
    strategy: &'static str,
}

fn estimate_for(state: &AppState, form: &SearchForm) -> EstimatePartial {
    let empty = |message: String| EstimatePartial {
        ok: false,
        message,
        live: 0,
        candidates: 0,
        scan: 0,
        per_day: 0,
        strategy: "",
    };
    let def = match form.to_definition(&state.engine.geo) {
        Ok(def) => def,
        Err(errors) => {
            return empty(format!(
                "Stima disponibile quando il modulo è valido ({}).",
                errors.summary()
            ));
        }
    };
    match state.engine.planner().estimate(&def) {
        Ok(estimate) => {
            let cycles = (24 * 60) as f64 / f64::from(def.policy.poll_interval_minutes.max(1));
            EstimatePartial {
                ok: true,
                message: String::new(),
                live: estimate.live_per_cycle,
                candidates: estimate.candidates,
                scan: estimate.scan_per_cycle,
                per_day: (estimate.live_per_cycle as f64 * cycles).round() as usize,
                strategy: match estimate.strategy {
                    Strategy::All => "Tutte le combinazioni rientrano nel budget.",
                    Strategy::Budget => {
                        "Oltre il budget: ogni ciclo controlla prima le combinazioni più convenienti e ruota le altre."
                    }
                    Strategy::ScanVerify => {
                        "Scan sui prezzi in cache, poi verifica live solo delle combinazioni migliori."
                    }
                },
            }
        }
        Err(error) => empty(error.to_string()),
    }
}

fn form_page(
    state: &AppState,
    form: SearchForm,
    errors: FormErrors,
    editing: Option<i64>,
) -> FormPage {
    let estimate = estimate_for(state, &form).render().unwrap_or_default();
    FormPage {
        nav: "new",
        title: if editing.is_some() {
            "Modifica ricerca".into()
        } else {
            "Nuova ricerca".into()
        },
        action: editing.map_or_else(|| "/searches".into(), |id| format!("/searches/{id}")),
        submit: if editing.is_some() {
            "Salva modifiche"
        } else {
            "Salva e avvia"
        },
        cancel: editing.map_or_else(|| "/".into(), |id| format!("/searches/{id}")),
        form,
        errors,
        estimate,
        llm: state.llm.is_some(),
        notice: None,
    }
}

pub async fn new_form(State(state): State<AppState>) -> AppResult<Html<String>> {
    render(&form_page(
        &state,
        SearchForm::blank(state.defaults),
        FormErrors::default(),
        None,
    ))
}

pub async fn edit_form(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Html<String>> {
    let record = state.engine.db.get_search(SearchId(id)).await?;
    let form = SearchForm::from_definition(&record.search.definition);
    render(&form_page(&state, form, FormErrors::default(), Some(id)))
}

fn invalid(page: &FormPage) -> AppResult<Response> {
    Ok((StatusCode::UNPROCESSABLE_ENTITY, render(page)?).into_response())
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<SearchForm>,
) -> AppResult<Response> {
    let def = match form.to_definition(&state.engine.geo) {
        Ok(def) => def,
        Err(errors) => return invalid(&form_page(&state, form, errors, None)),
    };
    let id = state.engine.db.create_search(&def).await?;
    let record = state.engine.db.get_search(id).await?;
    state.start_run(record.search);
    Ok(Redirect::to(&format!("/searches/{}", id.0)).into_response())
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<SearchForm>,
) -> AppResult<Response> {
    let def = match form.to_definition(&state.engine.geo) {
        Ok(def) => def,
        Err(errors) => return invalid(&form_page(&state, form, errors, Some(id))),
    };
    state.engine.db.update_search(SearchId(id), &def).await?;
    Ok(Redirect::to(&format!("/searches/{id}")).into_response())
}

pub async fn estimate(
    State(state): State<AppState>,
    Form(form): Form<SearchForm>,
) -> AppResult<Html<String>> {
    render(&estimate_for(&state, &form))
}

#[derive(Deserialize)]
pub struct ParseRequest {
    text: String,
}

pub async fn parse_text(
    State(state): State<AppState>,
    Form(request): Form<ParseRequest>,
) -> AppResult<Response> {
    let Some(llm) = &state.llm else {
        return Err(AppError::NotFound);
    };
    let today = Local::now().date_naive();
    let mut page = match llm.parse(&request.text, today, state.defaults).await {
        Ok(form) => {
            let errors = match form.to_definition(&state.engine.geo) {
                Ok(_) => FormErrors::default(),
                Err(errors) => errors,
            };
            let mut page = form_page(&state, form, errors, None);
            page.notice =
                Some("Campi compilati dal modello locale: controllali prima di salvare.".into());
            page
        }
        Err(error) => {
            let mut page = form_page(
                &state,
                SearchForm::blank(state.defaults),
                FormErrors::default(),
                None,
            );
            page.notice = Some(format!(
                "Non sono riuscito a interpretare la richiesta: {error}"
            ));
            page
        }
    };
    page.nav = "new";
    Ok(render(&page)?.into_response())
}

// ---------- actions ----------

#[derive(Deserialize)]
pub struct BackTo {
    #[serde(default)]
    back: String,
}

fn back_to(back: &str, id: i64) -> Redirect {
    if back == "/" {
        Redirect::to("/")
    } else {
        Redirect::to(&format!("/searches/{id}"))
    }
}

pub async fn toggle(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(back): Form<BackTo>,
) -> AppResult<Redirect> {
    let record = state.engine.db.get_search(SearchId(id)).await?;
    state
        .engine
        .db
        .set_search_active(SearchId(id), !record.search.active)
        .await?;
    Ok(back_to(&back.back, id))
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Redirect> {
    state.engine.db.delete_search(SearchId(id)).await?;
    Ok(Redirect::to("/"))
}

#[derive(Template)]
#[template(path = "partials/run_status.html")]
struct RunStatus {
    id: i64,
}

pub async fn run_now(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(back): Form<BackTo>,
) -> AppResult<Response> {
    let record = state.engine.db.get_search(SearchId(id)).await?;
    state.start_run(record.search);
    if headers.contains_key("hx-request") {
        Ok(render(&RunStatus { id })?.into_response())
    } else {
        Ok(back_to(&back.back, id).into_response())
    }
}

pub async fn run_status(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    if state.is_running(SearchId(id)) {
        Ok(render(&RunStatus { id })?.into_response())
    } else {
        Ok(([("HX-Refresh", "true")], Html(String::new())).into_response())
    }
}

// ---------- geo ----------

struct Chip {
    code: String,
    name: String,
}

struct SuggestionView {
    value: String,
    label: String,
}

#[derive(Template)]
#[template(path = "partials/preview.html")]
struct Preview {
    prefix: String,
    chips: Vec<Chip>,
    more: usize,
    message: Option<String>,
    suggestions: Vec<SuggestionView>,
}

const PREVIEW_CHIPS: usize = 12;

fn chips(codes: &[Iata], geo: &GeoIndex) -> Vec<Chip> {
    codes
        .iter()
        .take(PREVIEW_CHIPS)
        .map(|code| Chip {
            code: code.to_string(),
            name: geo
                .airport(code)
                .map(|a| a.city.clone())
                .unwrap_or_default(),
        })
        .collect()
}

pub async fn geo_preview(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<Html<String>> {
    let geo = &state.engine.geo;
    let prefix = params.get("prefix").cloned().unwrap_or_default();
    let field = |suffix: &str| {
        params
            .get(&format!("{prefix}_{suffix}"))
            .cloned()
            .unwrap_or_default()
    };
    let (kind, value) = (field("kind"), field("value"));
    let last_term = value
        .rsplit([',', ';'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    let wanted = match kind.as_str() {
        "airports" => Some(SuggestionKind::Airport),
        "city" => Some(SuggestionKind::City),
        "country" => Some(SuggestionKind::Country),
        _ => None,
    };
    let suggestions = geo
        .suggest(&last_term, 30)
        .into_iter()
        .filter(|s| {
            wanted == Some(s.kind)
                || (wanted == Some(SuggestionKind::City) && s.kind == SuggestionKind::Airport)
        })
        .take(8)
        .map(|s| {
            let value = if kind == "airports" {
                let mut parts: Vec<&str> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .collect();
                parts.pop();
                parts.push(&s.value);
                parts.join(", ")
            } else if s.kind == SuggestionKind::Airport {
                s.detail.split(',').next().unwrap_or_default().to_string()
            } else {
                s.value.clone()
            };
            SuggestionView {
                value,
                label: format!("{} · {}", s.label, s.detail),
            }
        })
        .collect();
    let mut preview = Preview {
        prefix,
        chips: Vec::new(),
        more: 0,
        message: None,
        suggestions,
    };
    match parse_location(&kind, &value, geo) {
        Ok(Some(set)) => match geo.resolve(&set) {
            Ok(codes) => {
                preview.more = codes.len().saturating_sub(PREVIEW_CHIPS);
                preview.chips = chips(&codes, geo);
            }
            Err(geo::GeoError::OpenDestination) => {
                preview.message =
                    Some("Destinazioni scelte dallo scan dei prezzi in cache.".into());
            }
            Err(error) => preview.message = Some(error.to_string()),
        },
        Ok(None) => {}
        Err(message) => preview.message = Some(message),
    }
    render(&preview)
}

// ---------- providers ----------

struct RunRow {
    when: String,
    provider: String,
    description: String,
    ok: bool,
    outcome: String,
    results: i64,
    duration: String,
}

#[derive(Template)]
#[template(path = "providers.html")]
struct ProvidersPage {
    nav: &'static str,
    health: Vec<HealthView>,
    runs: Vec<RunRow>,
    telegram: bool,
    airports: usize,
}

fn outcome_label(outcome: &str) -> &'static str {
    match outcome {
        "ok" => "ok",
        "network" => "rete",
        "rate_limited" => "limite richieste",
        "blocked" => "bloccata",
        "parse" => "formato cambiato",
        "unsupported" => "non supportata",
        _ => "errore",
    }
}

pub async fn providers(State(state): State<AppState>) -> AppResult<Html<String>> {
    let now = Utc::now();
    let runs = state
        .engine
        .db
        .recent_query_runs(80)
        .await?
        .into_iter()
        .map(|r| RunRow {
            when: relative(r.started_at, now),
            provider: provider_label(&r.provider).to_string(),
            description: r.description,
            ok: r.outcome == "ok",
            outcome: match &r.error {
                Some(error) => format!("{}: {error}", outcome_label(&r.outcome)),
                None => outcome_label(&r.outcome).to_string(),
            },
            results: r.result_count,
            duration: if r.duration_ms >= 1000 {
                format!("{:.1} s", r.duration_ms as f64 / 1000.0)
            } else {
                format!("{} ms", r.duration_ms)
            },
        })
        .collect();
    render(&ProvidersPage {
        nav: "providers",
        health: provider_health(&state).await?,
        runs,
        telegram: state.engine.notifier().is_enabled(),
        airports: state.engine.geo.airport_count(),
    })
}

pub async fn not_found() -> AppError {
    AppError::NotFound
}
