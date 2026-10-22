use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{Db, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum HealthStatus {
    Ok,
    Degraded,
}

impl HealthStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
        }
    }

    fn parse(value: &str) -> Self {
        if value == "degraded" {
            Self::Degraded
        } else {
            Self::Ok
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderHealth {
    pub provider: String,
    pub status: HealthStatus,
    pub consecutive_failures: i64,
    pub last_error: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl ProviderHealth {
    pub fn new(provider: &str) -> Self {
        Self {
            provider: provider.to_string(),
            status: HealthStatus::Ok,
            consecutive_failures: 0,
            last_error: None,
            last_success_at: None,
            last_failure_at: None,
            updated_at: Utc::now(),
        }
    }
}

impl Db {
    pub async fn provider_health(&self, provider: &str) -> Result<ProviderHealth> {
        let row = sqlx::query!(
            r#"SELECT provider, status, consecutive_failures, last_error,
                      last_success_at AS "last_success_at: DateTime<Utc>",
                      last_failure_at AS "last_failure_at: DateTime<Utc>",
                      updated_at AS "updated_at: DateTime<Utc>"
               FROM provider_health WHERE provider = ?"#,
            provider
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map_or_else(
            || ProviderHealth::new(provider),
            |r| ProviderHealth {
                provider: r.provider,
                status: HealthStatus::parse(&r.status),
                consecutive_failures: r.consecutive_failures,
                last_error: r.last_error,
                last_success_at: r.last_success_at,
                last_failure_at: r.last_failure_at,
                updated_at: r.updated_at,
            },
        ))
    }

    pub async fn all_provider_health(&self) -> Result<Vec<ProviderHealth>> {
        let providers =
            sqlx::query_scalar!("SELECT provider FROM provider_health ORDER BY provider")
                .fetch_all(&self.pool)
                .await?;
        let mut all = Vec::with_capacity(providers.len());
        for provider in providers {
            all.push(self.provider_health(&provider).await?);
        }
        Ok(all)
    }

    pub async fn save_provider_health(&self, health: &ProviderHealth) -> Result<()> {
        let status = health.status.as_str();
        sqlx::query!(
            "INSERT INTO provider_health
                (provider, status, consecutive_failures, last_error, last_success_at,
                 last_failure_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (provider) DO UPDATE SET
                status = excluded.status,
                consecutive_failures = excluded.consecutive_failures,
                last_error = excluded.last_error,
                last_success_at = excluded.last_success_at,
                last_failure_at = excluded.last_failure_at,
                updated_at = excluded.updated_at",
            health.provider,
            status,
            health.consecutive_failures,
            health.last_error,
            health.last_success_at,
            health.last_failure_at,
            health.updated_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
