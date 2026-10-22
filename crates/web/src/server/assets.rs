use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

struct Asset {
    name: &'static str,
    mime: &'static str,
    body: &'static [u8],
}

macro_rules! asset {
    ($name:literal, $mime:literal) => {
        Asset {
            name: $name,
            mime: $mime,
            body: include_bytes!(concat!("../../static/", $name)),
        }
    };
}

/// Everything the UI needs ships inside the binary, so it works offline.
const ASSETS: &[Asset] = &[
    asset!("app.css", "text/css; charset=utf-8"),
    asset!("app.js", "text/javascript; charset=utf-8"),
    asset!("htmx.min.js", "text/javascript; charset=utf-8"),
    asset!("icons.css", "text/css; charset=utf-8"),
    asset!("icons.woff2", "font/woff2"),
    asset!("Geist-Variable.woff2", "font/woff2"),
    asset!("GeistMono-Variable.woff2", "font/woff2"),
];

pub async fn serve(Path(name): Path<String>) -> Response {
    match ASSETS.iter().find(|a| a.name == name) {
        Some(asset) => (
            [
                (header::CONTENT_TYPE, asset.mime),
                (header::CACHE_CONTROL, "public, max-age=3600"),
            ],
            asset.body,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
