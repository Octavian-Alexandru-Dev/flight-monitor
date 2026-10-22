use chrono::{DateTime, Utc};
use domain::{CalendarCell, Decimal, SearchId, TripOption};

use crate::{Db, Result, StorageError, from_cents, to_cents};

pub struct NewSearchRun<'a> {
    pub search_id: SearchId,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub queries_planned: u32,
    pub queries_executed: u32,
    pub queries_failed: u32,
    pub currency: &'a str,
    pub options: &'a [TripOption],
    pub calendar: &'a [CalendarCell],
}

#[derive(Clone, Debug)]
pub struct SearchRun {
    pub id: i64,
    pub search_id: SearchId,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub queries_planned: i64,
    pub queries_executed: i64,
    pub queries_failed: i64,
    pub best_price: Option<Decimal>,
    pub currency: String,
    pub options: Vec<TripOption>,
    pub calendar: Vec<CalendarCell>,
}

/// One point of a search's price history.
#[derive(Clone, Debug, PartialEq)]
pub struct RunPoint {
    pub run_id: i64,
    pub at: DateTime<Utc>,
    pub best_price: Option<Decimal>,
}

struct Row {
    id: i64,
    search_id: i64,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    queries_planned: i64,
    queries_executed: i64,
    queries_failed: i64,
    best_price_cents: Option<i64>,
    currency: String,
    options: String,
    calendar: String,
}

impl TryFrom<Row> for SearchRun {
    type Error = StorageError;

    fn try_from(row: Row) -> Result<Self> {
        Ok(Self {
            id: row.id,
            search_id: SearchId(row.search_id),
            started_at: row.started_at,
            finished_at: row.finished_at,
            queries_planned: row.queries_planned,
            queries_executed: row.queries_executed,
            queries_failed: row.queries_failed,
            best_price: row.best_price_cents.map(from_cents),
            currency: row.currency,
            options: serde_json::from_str(&row.options)?,
            calendar: serde_json::from_str(&row.calendar)?,
        })
    }
}

impl Db {
    pub async fn record_search_run(&self, run: NewSearchRun<'_>) -> Result<i64> {
        let best = run
            .options
            .iter()
            .map(|o| o.total_price)
            .min()
            .map(to_cents);
        let options = serde_json::to_string(run.options)?;
        let calendar = serde_json::to_string(run.calendar)?;
        Ok(sqlx::query!(
            "INSERT INTO search_runs
                (search_id, started_at, finished_at, queries_planned, queries_executed,
                 queries_failed, best_price_cents, currency, options, calendar)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            run.search_id.0,
            run.started_at,
            run.finished_at,
            run.queries_planned,
            run.queries_executed,
            run.queries_failed,
            best,
            run.currency,
            options,
            calendar
        )
        .execute(&self.pool)
        .await?
        .last_insert_rowid())
    }

    pub async fn latest_search_run(&self, search_id: SearchId) -> Result<Option<SearchRun>> {
        sqlx::query_as!(
            Row,
            r#"SELECT id AS "id!", search_id, started_at AS "started_at: DateTime<Utc>",
                      finished_at AS "finished_at: DateTime<Utc>", queries_planned,
                      queries_executed, queries_failed, best_price_cents, currency, options, calendar
               FROM search_runs WHERE search_id = ? ORDER BY started_at DESC, id DESC LIMIT 1"#,
            search_id.0
        )
        .fetch_optional(&self.pool)
        .await?
        .map(SearchRun::try_from)
        .transpose()
    }

    /// Price history of a search, oldest first.
    pub async fn run_points(&self, search_id: SearchId, limit: i64) -> Result<Vec<RunPoint>> {
        let mut rows = sqlx::query!(
            r#"SELECT id AS "id!", started_at AS "started_at: DateTime<Utc>", best_price_cents
               FROM search_runs WHERE search_id = ? ORDER BY started_at DESC, id DESC LIMIT ?"#,
            search_id.0,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        rows.reverse();
        Ok(rows
            .into_iter()
            .map(|r| RunPoint {
                run_id: r.id,
                at: r.started_at,
                best_price: r.best_price_cents.map(from_cents),
            })
            .collect())
    }

    /// Lowest best price ever recorded for the search, excluding one run.
    pub async fn historic_min(
        &self,
        search_id: SearchId,
        except_run: i64,
    ) -> Result<Option<Decimal>> {
        let min = sqlx::query_scalar!(
            r#"SELECT MIN(best_price_cents) AS "min: i64" FROM search_runs
               WHERE search_id = ? AND id != ?"#,
            search_id.0,
            except_run
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(min.map(from_cents))
    }

    /// Best price of the most recent run before `run_id` that found something.
    pub async fn previous_best(&self, search_id: SearchId, run_id: i64) -> Result<Option<Decimal>> {
        let previous = sqlx::query_scalar!(
            r#"SELECT best_price_cents AS "best!: i64" FROM search_runs
               WHERE search_id = ? AND id < ? AND best_price_cents IS NOT NULL
               ORDER BY id DESC LIMIT 1"#,
            search_id.0,
            run_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(previous.map(from_cents))
    }
}
