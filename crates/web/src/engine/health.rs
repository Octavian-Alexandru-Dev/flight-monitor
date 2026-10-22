use alerts::{Notifier, health_message};
use chrono::Utc;
use providers::ProviderError;
use storage::{Db, HealthStatus};

/// Consecutive structural failures (parse, block, suspicious silence) before degrading.
const STRUCTURAL_LIMIT: i64 = 3;
/// Consecutive failures of any kind before degrading.
const ANY_LIMIT: i64 = 5;

pub enum HealthEvent {
    Answered,
    /// No results on a query that normally has some: possibly a silent breakage.
    SuspiciousEmpty,
    Failure(ProviderError),
}

pub async fn record(
    db: &Db,
    notifier: &dyn Notifier,
    provider: &str,
    event: HealthEvent,
) -> anyhow::Result<()> {
    if matches!(event, HealthEvent::Failure(ProviderError::Unsupported)) {
        return Ok(());
    }
    let mut health = db.provider_health(provider).await?;
    let before = health.status;
    let now = Utc::now();
    health.updated_at = now;
    match event {
        HealthEvent::Answered => {
            health.consecutive_failures = 0;
            health.status = HealthStatus::Ok;
            health.last_success_at = Some(now);
        }
        HealthEvent::SuspiciousEmpty | HealthEvent::Failure(_) => {
            let (structural, detail) = match &event {
                HealthEvent::Failure(error) => (error.is_structural(), error.to_string()),
                _ => (
                    true,
                    "nessun risultato su una rotta che di solito ne ha".to_string(),
                ),
            };
            health.consecutive_failures += 1;
            health.last_failure_at = Some(now);
            health.last_error = Some(detail);
            let limit = if structural {
                STRUCTURAL_LIMIT
            } else {
                ANY_LIMIT
            };
            if health.consecutive_failures >= limit {
                health.status = HealthStatus::Degraded;
            }
        }
    }
    db.save_provider_health(&health).await?;
    if before != health.status {
        let degraded = health.status == HealthStatus::Degraded;
        tracing::warn!(provider, degraded, "provider health changed");
        let detail = health.last_error.clone().unwrap_or_default();
        if let Err(error) = notifier
            .send(&health_message(provider, degraded, &detail))
            .await
        {
            tracing::warn!(%error, "cannot send health notification");
        }
    }
    Ok(())
}
