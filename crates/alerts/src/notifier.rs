use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("telegram request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("telegram rejected the message: {0}")]
    Rejected(String),
}

/// Delivers HTML-formatted messages to the user.
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn send(&self, html: &str) -> Result<(), NotifyError>;

    fn is_enabled(&self) -> bool {
        true
    }
}

pub struct TelegramNotifier {
    client: reqwest::Client,
    endpoint: String,
    chat_id: String,
}

#[derive(Deserialize)]
struct TelegramReply {
    ok: bool,
    description: Option<String>,
}

impl TelegramNotifier {
    pub fn new(token: &str, chat_id: &str) -> Result<Self, NotifyError> {
        Self::with_base("https://api.telegram.org", token, chat_id)
    }

    pub fn with_base(base: &str, token: &str, chat_id: &str) -> Result<Self, NotifyError> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()?,
            endpoint: format!("{base}/bot{token}/sendMessage"),
            chat_id: chat_id.to_string(),
        })
    }
}

#[async_trait]
impl Notifier for TelegramNotifier {
    async fn send(&self, html: &str) -> Result<(), NotifyError> {
        let reply: TelegramReply = self
            .client
            .post(&self.endpoint)
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "text": html,
                "parse_mode": "HTML",
                "link_preview_options": { "is_disabled": true },
            }))
            .send()
            .await?
            .json()
            .await?;
        if reply.ok {
            Ok(())
        } else {
            Err(NotifyError::Rejected(reply.description.unwrap_or_default()))
        }
    }
}

/// Used when Telegram is not configured: alerts only reach the log and the web UI.
pub struct LogNotifier;

#[async_trait]
impl Notifier for LogNotifier {
    async fn send(&self, html: &str) -> Result<(), NotifyError> {
        tracing::info!(message = html, "notification (Telegram not configured)");
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        false
    }
}

/// Keeps messages in memory; for tests and dry runs.
#[derive(Default)]
pub struct RecordingNotifier {
    sent: Mutex<Vec<String>>,
}

impl RecordingNotifier {
    pub fn messages(&self) -> Vec<String> {
        self.sent.lock().map(|m| m.clone()).unwrap_or_default()
    }
}

#[async_trait]
impl Notifier for RecordingNotifier {
    async fn send(&self, html: &str) -> Result<(), NotifyError> {
        if let Ok(mut sent) = self.sent.lock() {
            sent.push(html.to_string());
        }
        Ok(())
    }
}
