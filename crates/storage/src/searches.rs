use chrono::{DateTime, Utc};
use domain::{SavedSearch, SearchDefinition, SearchId};

use crate::{Db, Result, StorageError};

#[derive(Clone, Debug)]
pub struct SearchRecord {
    pub search: SavedSearch,
    pub created_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
}

struct Row {
    id: i64,
    definition: String,
    active: bool,
    created_at: DateTime<Utc>,
    last_run_at: Option<DateTime<Utc>>,
    next_run_at: Option<DateTime<Utc>>,
}

impl TryFrom<Row> for SearchRecord {
    type Error = StorageError;

    fn try_from(row: Row) -> Result<Self> {
        Ok(Self {
            search: SavedSearch {
                id: SearchId(row.id),
                active: row.active,
                definition: serde_json::from_str(&row.definition)?,
            },
            created_at: row.created_at,
            last_run_at: row.last_run_at,
            next_run_at: row.next_run_at,
        })
    }
}

impl Db {
    pub async fn create_search(&self, definition: &SearchDefinition) -> Result<SearchId> {
        let json = serde_json::to_string(definition)?;
        let now = Utc::now();
        let id = sqlx::query!(
            "INSERT INTO saved_searches (name, definition, active, created_at, updated_at)
             VALUES (?, ?, 1, ?, ?)",
            definition.name,
            json,
            now,
            now
        )
        .execute(&self.pool)
        .await?
        .last_insert_rowid();
        Ok(SearchId(id))
    }

    /// Saves a new definition and makes the search due immediately.
    pub async fn update_search(&self, id: SearchId, definition: &SearchDefinition) -> Result<()> {
        let json = serde_json::to_string(definition)?;
        let now = Utc::now();
        let affected = sqlx::query!(
            "UPDATE saved_searches SET name = ?, definition = ?, updated_at = ?, next_run_at = NULL
             WHERE id = ?",
            definition.name,
            json,
            now,
            id.0
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    pub async fn set_search_active(&self, id: SearchId, active: bool) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            "UPDATE saved_searches SET active = ?, updated_at = ?, next_run_at = NULL WHERE id = ?",
            active,
            now,
            id.0
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_search(&self, id: SearchId) -> Result<()> {
        sqlx::query!("DELETE FROM saved_searches WHERE id = ?", id.0)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_search(&self, id: SearchId) -> Result<SearchRecord> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT id AS "id!", definition, active AS "active: bool",
                      created_at AS "created_at: DateTime<Utc>",
                      last_run_at AS "last_run_at: DateTime<Utc>",
                      next_run_at AS "next_run_at: DateTime<Utc>"
               FROM saved_searches WHERE id = ?"#,
            id.0
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StorageError::NotFound)?;
        row.try_into()
    }

    pub async fn list_searches(&self) -> Result<Vec<SearchRecord>> {
        sqlx::query_as!(
            Row,
            r#"SELECT id AS "id!", definition, active AS "active: bool",
                      created_at AS "created_at: DateTime<Utc>",
                      last_run_at AS "last_run_at: DateTime<Utc>",
                      next_run_at AS "next_run_at: DateTime<Utc>"
               FROM saved_searches ORDER BY active DESC, name"#
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(SearchRecord::try_from)
        .collect()
    }

    pub async fn due_searches(&self, now: DateTime<Utc>) -> Result<Vec<SearchRecord>> {
        Ok(self
            .list_searches()
            .await?
            .into_iter()
            .filter(|r| r.search.active && r.next_run_at.is_none_or(|next| next <= now))
            .collect())
    }

    pub async fn mark_search_ran(
        &self,
        id: SearchId,
        ran_at: DateTime<Utc>,
        next_run_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE saved_searches SET last_run_at = ?, next_run_at = ? WHERE id = ?",
            ran_at,
            next_run_at,
            id.0
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
