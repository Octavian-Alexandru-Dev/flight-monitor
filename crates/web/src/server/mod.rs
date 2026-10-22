mod assets;
mod charts;
mod form;
mod llm;
mod pages;
mod views;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use askama::Template;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use domain::SearchId;

use crate::engine::Engine;
pub use form::{FormDefaults, SearchForm};
pub use llm::LlmClient;

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub defaults: FormDefaults,
    pub llm: Option<Arc<LlmClient>>,
    running: Arc<Mutex<HashSet<SearchId>>>,
}

impl AppState {
    pub fn new(engine: Arc<Engine>, defaults: FormDefaults, llm: Option<LlmClient>) -> Self {
        Self {
            engine,
            defaults,
            llm: llm.map(Arc::new),
            running: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn is_running(&self, id: SearchId) -> bool {
        self.running.lock().is_ok_and(|r| r.contains(&id))
    }

    /// Starts a background run unless one is already going; returns whether it started.
    fn start_run(&self, search: domain::SavedSearch) -> bool {
        let Ok(mut running) = self.running.lock() else {
            return false;
        };
        if !running.insert(search.id) {
            return false;
        }
        let state = self.clone();
        tokio::spawn(async move {
            let id = search.id;
            if let Err(error) = state.engine.run_searches(vec![search]).await {
                tracing::error!(%error, "manual run failed");
            }
            if let Ok(mut running) = state.running.lock() {
                running.remove(&id);
            }
        });
        true
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(pages::dashboard))
        .route("/searches", post(pages::create))
        .route("/searches/new", get(pages::new_form))
        .route("/searches/estimate", post(pages::estimate))
        .route("/searches/parse", post(pages::parse_text))
        .route("/searches/{id}", get(pages::detail).post(pages::update))
        .route("/searches/{id}/edit", get(pages::edit_form))
        .route("/searches/{id}/toggle", post(pages::toggle))
        .route("/searches/{id}/delete", post(pages::delete))
        .route("/searches/{id}/run", post(pages::run_now))
        .route("/searches/{id}/status", get(pages::run_status))
        .route("/geo/preview", get(pages::geo_preview))
        .route("/providers", get(pages::providers))
        .route("/static/{name}", get(assets::serve))
        .fallback(pages::not_found)
        .with_state(state)
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage {
    nav: &'static str,
    title: &'static str,
    message: String,
}

#[derive(Debug)]
pub enum AppError {
    NotFound,
    Internal(anyhow::Error),
}

impl From<storage::StorageError> for AppError {
    fn from(error: storage::StorageError) -> Self {
        match error {
            storage::StorageError::NotFound => Self::NotFound,
            other => Self::Internal(other.into()),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

impl From<askama::Error> for AppError {
    fn from(error: askama::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, page) = match self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                ErrorPage {
                    nav: "",
                    title: "Pagina non trovata",
                    message: "La ricerca o la pagina richiesta non esiste più.".into(),
                },
            ),
            Self::Internal(error) => {
                tracing::error!(error = format!("{error:#}"), "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorPage {
                        nav: "",
                        title: "Qualcosa è andato storto",
                        message: format!("{error:#}"),
                    },
                )
            }
        };
        match page.render() {
            Ok(html) => (status, Html(html)).into_response(),
            Err(_) => status.into_response(),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

fn render<T: Template>(template: &T) -> AppResult<Html<String>> {
    Ok(Html(template.render()?))
}
