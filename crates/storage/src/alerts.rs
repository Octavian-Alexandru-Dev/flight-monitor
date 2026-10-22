use chrono::{DateTime, Utc};
use domain::{Decimal, SearchId};

use crate::{Db, Result, from_cents, to_cents};

pub struct NewAlert<'a> {
    pub search_id: SearchId,
    pub option_key: &'a str,
    pub price: Decimal,
    pub reasons: &'a str,
    pub message: &'a str,
}

#[derive(Clone, Debug)]
pub struct AlertRecord {
    pub search_id: SearchId,
    pub price: Decimal,
    pub reasons: String,
    pub sent_at: DateTime<Utc>,
}

impl Db {
    /// True when the same option was already notified at this price or lower.
    pub async fn alert_already_sent(
        &self,
        search_id: SearchId,
        option_key: &str,
        price: Decimal,
    ) -> Result<bool> {
        let cents = to_cents(price);
        let hits = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM alerts_sent
               WHERE search_id = ? AND option_key = ? AND price_cents <= ?"#,
            search_id.0,
            option_key,
            cents
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(hits > 0)
    }

    pub async fn record_alert(&self, alert: NewAlert<'_>) -> Result<()> {
        let cents = to_cents(alert.price);
        let now = Utc::now();
        sqlx::query!(
            "INSERT INTO alerts_sent (search_id, option_key, price_cents, reasons, message, sent_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            alert.search_id.0,
            alert.option_key,
            cents,
            alert.reasons,
            alert.message,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn recent_alerts(&self, limit: i64) -> Result<Vec<AlertRecord>> {
        let rows = sqlx::query!(
            r#"SELECT search_id, price_cents, reasons, sent_at AS "sent_at: DateTime<Utc>"
               FROM alerts_sent ORDER BY sent_at DESC LIMIT ?"#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| AlertRecord {
                search_id: SearchId(r.search_id),
                price: from_cents(r.price_cents),
                reasons: r.reasons,
                sent_at: r.sent_at,
            })
            .collect())
    }
}
