//! PostgreSQL [`TagMuteRuleRepository`] adapter.

use super::{Db, now};
use crate::domain::{Id, TagMatch, TagMuteRule, Timestamp};
use crate::error::Result;
use crate::ports::{NewTagMuteRule, TagMuteRuleRepository};
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct PgTagMuteRuleRepository {
    db: Db,
}

impl PgTagMuteRuleRepository {
    pub fn new(db: Db) -> Self {
        PgTagMuteRuleRepository { db }
    }

    /// Load the tag pairs for a single rule, ordered by key.
    async fn tags_for(&self, rule_id: Id) -> Result<Vec<TagMatch>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT tag_key, tag_value FROM tag_mute_rule_tags \
             WHERE rule_id = $1 ORDER BY tag_key",
        )
        .bind(rule_id)
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(key, value)| TagMatch { key, value })
            .collect())
    }
}

#[derive(FromRow)]
struct RuleRow {
    id: Id,
    project_id: Id,
    name: Option<String>,
    created_by: Option<Id>,
    created_at: Timestamp,
    updated_at: Timestamp,
}

#[async_trait]
impl TagMuteRuleRepository for PgTagMuteRuleRepository {
    async fn create(&self, new: NewTagMuteRule) -> Result<TagMuteRule> {
        let id = Id::new_v4();
        let now = now();

        // The rule and its tags land atomically.
        let mut tx = self.db.begin().await?;

        sqlx::query(
            "INSERT INTO tag_mute_rules (id, project_id, name, created_by, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $5)",
        )
        .bind(id)
        .bind(new.project_id)
        .bind(new.name.as_deref())
        .bind(new.created_by)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        for tag in &new.tags {
            sqlx::query(
                "INSERT INTO tag_mute_rule_tags (rule_id, tag_key, tag_value) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(&tag.key)
            .bind(&tag.value)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(TagMuteRule {
            id,
            project_id: new.project_id,
            name: new.name,
            tags: new.tags,
            created_by: new.created_by,
            created_at: now,
            updated_at: now,
        })
    }

    async fn list_for_project(&self, project_id: Id) -> Result<Vec<TagMuteRule>> {
        let rows = sqlx::query_as::<_, RuleRow>(
            "SELECT id, project_id, name, created_by, created_at, updated_at \
             FROM tag_mute_rules WHERE project_id = $1 ORDER BY created_at DESC, id",
        )
        .bind(project_id)
        .fetch_all(&self.db)
        .await?;

        let mut rules = Vec::with_capacity(rows.len());
        for row in rows {
            rules.push(TagMuteRule {
                id: row.id,
                project_id: row.project_id,
                name: row.name,
                tags: self.tags_for(row.id).await?,
                created_by: row.created_by,
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }
        Ok(rules)
    }

    async fn delete(&self, project_id: Id, rule_id: Id) -> Result<bool> {
        // Scope the delete to the project so a rule from another project cannot
        // be removed by id. Child tags cascade (ON DELETE CASCADE).
        let result = sqlx::query("DELETE FROM tag_mute_rules WHERE id = $1 AND project_id = $2")
            .bind(rule_id)
            .bind(project_id)
            .execute(&self.db)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
