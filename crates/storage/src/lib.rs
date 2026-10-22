mod alerts;
mod health;
mod money;
mod query_runs;
mod search_runs;
mod searches;
mod state;

use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use thiserror::Error;

pub use alerts::{AlertRecord, NewAlert};
pub use health::{HealthStatus, ProviderHealth};
pub use money::{from_cents, to_cents};
pub use query_runs::{NewQueryRun, QueryHistory, QueryRunRecord, RouteStats};
pub use search_runs::{NewSearchRun, RunPoint, SearchRun};
pub use searches::SearchRecord;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("corrupt stored JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found")]
    NotFound,
}

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Opens (creating if needed) the database and applies pending migrations.
    pub async fn connect(url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        Self::migrate(pool).await
    }

    pub async fn in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Self::migrate(pool).await
    }

    async fn migrate(pool: SqlitePool) -> Result<Self> {
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}
