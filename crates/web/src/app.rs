use std::sync::Arc;

use alerts::{LogNotifier, Notifier, TelegramNotifier};
use anyhow::Context;
use geo::GeoIndex;
use storage::Db;

use crate::config::Config;
use crate::engine::{Engine, EngineSettings};

pub async fn open_db(config: &Config) -> anyhow::Result<Db> {
    if let Some(path) = config.database_url.strip_prefix("sqlite://")
        && let Some(parent) = std::path::Path::new(path).parent()
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    Db::connect(&config.database_url)
        .await
        .with_context(|| format!("opening {}", config.database_url))
}

pub async fn engine(config: &Config) -> anyhow::Result<Arc<Engine>> {
    let db = open_db(config).await?;
    let geo = GeoIndex::load(&config.data_dir)
        .with_context(|| format!("loading reference data from {}", config.data_dir.display()))?;
    let notifier: Arc<dyn Notifier> = match (&config.telegram_token, &config.telegram_chat_id) {
        (Some(token), Some(chat)) if !token.is_empty() && !chat.is_empty() => {
            Arc::new(TelegramNotifier::new(token, chat)?)
        }
        _ => {
            tracing::warn!("Telegram not configured: alerts are only logged and shown in the UI");
            Arc::new(LogNotifier)
        }
    };
    Ok(Arc::new(Engine::new(
        db,
        Arc::new(geo),
        config.google_flights(),
        config.travelpayouts()?,
        notifier,
        EngineSettings {
            cache_ttl: chrono::Duration::minutes(config.cache_ttl_minutes),
            verify_top_n: config.verify_top_n,
            public_url: config.public_url.clone(),
        },
    )))
}
