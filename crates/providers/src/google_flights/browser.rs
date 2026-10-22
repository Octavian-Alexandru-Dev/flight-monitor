//! Headless Chromium for searches Google has not cached: the page loads the results
//! through an RPC signed by its own JavaScript, which plain HTTP cannot reproduce.

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, GetResponseBodyParams, RequestPattern,
    RequestStage,
};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::detection::{DetectionOptions, default_executable};
use chromiumoxide::fetcher::{BrowserFetcher, BrowserFetcherOptions};
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::OnceCell;

use super::page::{embedded_payload, flight_count, rpc_payloads};
use super::{CONSENT_COOKIE, CONSENT_VALUE};
use crate::ProviderError;

const RESULTS_RPC: &str = "*GetShoppingResults*";
const TIMEOUT: Duration = Duration::from_secs(40);

fn browser_error(error: impl std::fmt::Display) -> ProviderError {
    ProviderError::Network(format!("browser: {error}"))
}

/// Where Chromium comes from: an explicit path, an installed Chrome, or a download.
pub struct ChromeSource {
    pub executable: Option<PathBuf>,
    pub download_dir: PathBuf,
}

pub struct HeadlessBrowser {
    source: ChromeSource,
    executable: OnceCell<PathBuf>,
}

impl HeadlessBrowser {
    pub fn new(source: ChromeSource) -> Self {
        Self {
            source,
            executable: OnceCell::new(),
        }
    }

    /// Finds or downloads Chromium once; returns its path.
    pub async fn executable(&self) -> Result<PathBuf, ProviderError> {
        self.executable
            .get_or_try_init(|| async {
                if let Some(path) = &self.source.executable {
                    return Ok(path.clone());
                }
                if let Ok(installed) = default_executable(DetectionOptions::default()) {
                    return Ok(installed);
                }
                tracing::debug!(dir = %self.source.download_dir.display(), "using the downloaded Chromium");
                tokio::fs::create_dir_all(&self.source.download_dir)
                    .await
                    .map_err(browser_error)?;
                let options = BrowserFetcherOptions::builder()
                    .with_path(&self.source.download_dir)
                    .build()
                    .map_err(browser_error)?;
                let installation = BrowserFetcher::new(options).fetch().await.map_err(browser_error)?;
                Ok(installation.executable_path)
            })
            .await
            .cloned()
    }

    /// Opens the results page and returns the richest payload it shows.
    pub async fn results(&self, url: &str) -> Result<Value, ProviderError> {
        let config = BrowserConfig::builder()
            .chrome_executable(self.executable().await?)
            .new_headless_mode()
            .build()
            .map_err(browser_error)?;
        let (mut browser, mut handler) = Browser::launch(config).await.map_err(browser_error)?;
        let events = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let outcome = tokio::time::timeout(TIMEOUT, capture(&browser, url)).await;
        let _ = browser.close().await;
        events.abort();
        outcome
            .map_err(|_| ProviderError::Network("browser: results did not arrive in time".into()))?
    }
}

async fn capture(browser: &Browser, url: &str) -> Result<Value, ProviderError> {
    let consent = CookieParam::builder()
        .name(CONSENT_COOKIE)
        .value(CONSENT_VALUE)
        .domain(".google.com")
        .path("/")
        .build()
        .map_err(browser_error)?;
    browser
        .set_cookies(vec![consent])
        .await
        .map_err(browser_error)?;
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(browser_error)?;
    let mut paused = page
        .event_listener::<EventRequestPaused>()
        .await
        .map_err(browser_error)?;
    let pattern = RequestPattern::builder()
        .url_pattern(RESULTS_RPC)
        .request_stage(RequestStage::Response)
        .build();
    page.execute(EnableParams::builder().pattern(pattern).build())
        .await
        .map_err(browser_error)?;
    page.goto(url).await.map_err(browser_error)?;

    // The previous plain request may have warmed Google's cache: then the page embeds the results.
    if let Ok(html) = page.content().await
        && let Ok(Some(payload)) = embedded_payload(&html)
        && flight_count(&payload) > 0
    {
        return Ok(payload);
    }
    let mut seen = Vec::new();
    while let Some(event) = paused.next().await {
        let body = page
            .execute(GetResponseBodyParams::new(event.request_id.clone()))
            .await
            .map_err(browser_error)?;
        let _ = page
            .execute(ContinueRequestParams::new(event.request_id.clone()))
            .await;
        let text = if body.base64_encoded {
            let raw = STANDARD.decode(&body.body).map_err(browser_error)?;
            String::from_utf8_lossy(&raw).into_owned()
        } else {
            body.body.clone()
        };
        seen.extend(rpc_payloads(&text));
        if let Some(best) = seen.iter().max_by_key(|p| flight_count(p)) {
            return Ok(best.clone());
        }
    }
    let html = page.content().await.map_err(browser_error)?;
    embedded_payload(&html)?;
    Err(ProviderError::Parse(
        "results RPC not observed in the browser".into(),
    ))
}
