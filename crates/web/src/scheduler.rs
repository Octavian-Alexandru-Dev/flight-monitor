use std::sync::Arc;
use std::time::Duration;

use alerts::{SummaryLine, daily_summary};
use chrono::{Local, Timelike, Utc};
use storage::HealthStatus;

use crate::engine::Engine;

const TICK: Duration = Duration::from_secs(60);
const HOUSEKEEPING_KEY: &str = "housekeeping_day";
const SUMMARY_KEY: &str = "daily_summary_day";

pub struct SchedulerSettings {
    pub daily_summary_hour: Option<u32>,
    pub retention_days: i64,
}

/// Runs due searches every minute, plus daily housekeeping and the Telegram summary.
pub async fn run(engine: Arc<Engine>, settings: SchedulerSettings) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if let Err(error) = engine.run_due().await {
            tracing::error!(%error, "cycle failed");
        }
        if let Err(error) = daily(&engine, &settings).await {
            tracing::error!(%error, "daily tasks failed");
        }
    }
}

async fn daily(engine: &Engine, settings: &SchedulerSettings) -> anyhow::Result<()> {
    let now = Local::now();
    let today = now.date_naive().to_string();
    if engine.db.state(HOUSEKEEPING_KEY).await?.as_deref() != Some(today.as_str()) {
        let cutoff = Utc::now() - chrono::Duration::days(settings.retention_days);
        let pruned = engine.db.prune_query_runs(cutoff).await?;
        tracing::info!(pruned, "old query runs pruned");
        engine.db.set_state(HOUSEKEEPING_KEY, &today).await?;
    }
    let Some(hour) = settings.daily_summary_hour else {
        return Ok(());
    };
    if now.hour() >= hour && engine.db.state(SUMMARY_KEY).await?.as_deref() != Some(today.as_str())
    {
        let message = summary(engine).await?;
        engine.notifier().send(&message).await?;
        engine.db.set_state(SUMMARY_KEY, &today).await?;
    }
    Ok(())
}

pub async fn summary(engine: &Engine) -> anyhow::Result<String> {
    let day_ago = Utc::now() - chrono::Duration::hours(24);
    let mut lines = Vec::new();
    for record in engine.db.list_searches().await? {
        let search = &record.search;
        if !search.active {
            continue;
        }
        let points = engine.db.run_points(search.id, 500).await?;
        let best = points.last().and_then(|p| p.best_price);
        let day_before = points
            .iter()
            .rev()
            .find(|p| p.at <= day_ago)
            .and_then(|p| p.best_price);
        let history: Vec<_> = points.iter().filter_map(|p| p.best_price).collect();
        let days = (search.definition.outbound.first() - Utc::now().date_naive()).num_days();
        let verdict = best.map_or_else(
            || "nessun dato".to_string(),
            |price| {
                planner::advise(&history, price, days)
                    .verdict
                    .label()
                    .to_string()
            },
        );
        lines.push(SummaryLine {
            search: search.definition.name.clone(),
            best,
            day_before,
            currency: search.definition.currency.clone(),
            verdict,
        });
    }
    let degraded: Vec<String> = engine
        .db
        .all_provider_health()
        .await?
        .into_iter()
        .filter(|h| h.status == HealthStatus::Degraded)
        .map(|h| h.provider)
        .collect();
    Ok(daily_summary(&lines, &degraded))
}
