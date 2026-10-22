use std::collections::HashMap;

use chrono::{DateTime, Utc};
use domain::{Decimal, Iata, Itinerary, ProviderId, QueryStats};

use crate::{Db, Result, from_cents, to_cents};

pub struct NewQueryRun<'a> {
    pub provider: &'a ProviderId,
    pub query_key: &'a str,
    pub description: &'a str,
    pub started_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub outcome: &'a str,
    pub error: Option<&'a str>,
    pub itineraries: &'a [Itinerary],
}

#[derive(Clone, Debug)]
pub struct QueryRunRecord {
    pub id: i64,
    pub provider: String,
    pub description: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub outcome: String,
    pub error: Option<String>,
    pub result_count: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RouteStats {
    pub observations: i64,
    pub mean: Decimal,
    pub std_dev: Decimal,
}

impl Db {
    pub async fn record_query_run(&self, run: NewQueryRun<'_>) -> Result<i64> {
        let mut tx = self.pool.begin().await?;
        let provider = run.provider.as_str();
        let count = i64::try_from(run.itineraries.len()).unwrap_or(i64::MAX);
        let id = sqlx::query!(
            "INSERT INTO query_runs
                (provider, query_key, description, started_at, duration_ms, outcome, error, result_count)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            provider,
            run.query_key,
            run.description,
            run.started_at,
            run.duration_ms,
            run.outcome,
            run.error,
            count
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();

        for itinerary in run.itineraries {
            let key = itinerary.key();
            let origin = itinerary.origin().as_str();
            let destination = itinerary.destination().as_str();
            let departure = itinerary.departure_date();
            let cents = to_cents(itinerary.price);
            let source = itinerary.source.as_str();
            let json = serde_json::to_string(itinerary)?;
            sqlx::query!(
                "INSERT INTO price_observations
                    (query_run_id, itinerary_key, origin, destination, departure_date, return_date,
                     price_cents, currency, provider, observed_at, itinerary)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id,
                key,
                origin,
                destination,
                departure,
                itinerary.return_date,
                cents,
                itinerary.currency,
                source,
                itinerary.fetched_at,
                json
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Itineraries from the latest successful run of `query_key` newer than `since`.
    pub async fn cached_itineraries(
        &self,
        query_key: &str,
        since: DateTime<Utc>,
    ) -> Result<Option<Vec<Itinerary>>> {
        let run = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM query_runs
               WHERE query_key = ? AND outcome = 'ok' AND started_at >= ?
               ORDER BY started_at DESC LIMIT 1"#,
            query_key,
            since
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(run) = run else {
            return Ok(None);
        };
        let rows = sqlx::query_scalar!(
            "SELECT itinerary FROM price_observations WHERE query_run_id = ? ORDER BY id",
            run
        )
        .fetch_all(&self.pool)
        .await?;
        let itineraries = rows
            .iter()
            .map(|json| serde_json::from_str(json))
            .collect::<std::result::Result<Vec<Itinerary>, _>>()?;
        Ok(Some(itineraries))
    }

    pub async fn query_history(&self) -> Result<HashMap<String, QueryStats>> {
        let mut history: HashMap<String, QueryStats> = HashMap::new();
        let runs = sqlx::query!(
            r#"SELECT query_key AS "query_key!",
                      MAX(started_at) AS "last_run_at?: DateTime<Utc>",
                      COUNT(*) AS "runs!: i64",
                      SUM(result_count > 0) AS "with_results!: i64"
               FROM query_runs WHERE outcome = 'ok' GROUP BY query_key"#
        )
        .fetch_all(&self.pool)
        .await?;
        for row in runs {
            history.insert(
                row.query_key,
                QueryStats {
                    last_run_at: row.last_run_at,
                    runs: row.runs,
                    runs_with_results: row.with_results,
                    best_price: None,
                },
            );
        }
        let prices = sqlx::query!(
            r#"SELECT q.query_key AS "query_key!", MIN(p.price_cents) AS "best!: i64"
               FROM price_observations p JOIN query_runs q ON q.id = p.query_run_id
               GROUP BY q.query_key"#
        )
        .fetch_all(&self.pool)
        .await?;
        for row in prices {
            if let Some(entry) = history.get_mut(&row.query_key) {
                entry.best_price = Some(from_cents(row.best));
            }
        }
        Ok(history)
    }

    pub async fn route_stats(
        &self,
        origin: &Iata,
        destination: &Iata,
        currency: &str,
    ) -> Result<RouteStats> {
        let (origin, destination) = (origin.as_str(), destination.as_str());
        let row = sqlx::query!(
            r#"SELECT COUNT(*) AS "n!: i64",
                      AVG(price_cents) AS "mean: f64",
                      AVG(price_cents * price_cents) AS "mean_sq: f64"
               FROM price_observations
               WHERE origin = ? AND destination = ? AND currency = ? AND return_date IS NULL"#,
            origin,
            destination,
            currency
        )
        .fetch_one(&self.pool)
        .await?;
        let mean = row.mean.unwrap_or(0.0);
        let variance = (row.mean_sq.unwrap_or(0.0) - mean * mean).max(0.0);
        let cents = |value: f64| from_cents(value.round() as i64);
        Ok(RouteStats {
            observations: row.n,
            mean: cents(mean),
            std_dev: cents(variance.sqrt()),
        })
    }

    pub async fn recent_query_runs(&self, limit: i64) -> Result<Vec<QueryRunRecord>> {
        Ok(sqlx::query_as!(
            QueryRunRecord,
            r#"SELECT id AS "id!", provider, description,
                      started_at AS "started_at: DateTime<Utc>",
                      duration_ms, outcome, error, result_count
               FROM query_runs ORDER BY started_at DESC LIMIT ?"#,
            limit
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Whether the query has returned results before, so an empty answer is suspicious.
    pub async fn query_usually_has_results(&self, query_key: &str) -> Result<bool> {
        let hits = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM query_runs
               WHERE query_key = ? AND outcome = 'ok' AND result_count > 0"#,
            query_key
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(hits >= 2)
    }

    /// Drops raw observations older than `before`; search runs keep the history.
    pub async fn prune_query_runs(&self, before: DateTime<Utc>) -> Result<u64> {
        Ok(
            sqlx::query!("DELETE FROM query_runs WHERE started_at < ?", before)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }
}
