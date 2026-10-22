use crate::{Db, Result};

impl Db {
    pub async fn state(&self, key: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar!("SELECT value FROM app_state WHERE key = ?", key)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn set_state(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query!(
            "INSERT INTO app_state (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            key,
            value
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
