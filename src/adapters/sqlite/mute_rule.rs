//! SQLite [`TagMuteRuleRepository`] adapter.

use super::{Db, begin_write, id_from_db, id_to_db, ts_from_db, ts_to_db};
use crate::domain::{Id, TagMatch, TagMuteRule};
use crate::error::Result;
use crate::ports::{NewTagMuteRule, TagMuteRuleRepository};
use async_trait::async_trait;
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteTagMuteRuleRepository {
    db: Db,
}

impl SqliteTagMuteRuleRepository {
    pub fn new(db: Db) -> Self {
        SqliteTagMuteRuleRepository { db }
    }

    /// Load the tag pairs for a single rule, preserving insertion order.
    async fn tags_for(&self, rule_id: Id) -> Result<Vec<TagMatch>> {
        let rows = sqlx::query(
            "SELECT tag_key, tag_value FROM tag_mute_rule_tags \
             WHERE rule_id = ? ORDER BY tag_key",
        )
        .bind(id_to_db(rule_id))
        .fetch_all(&self.db)
        .await?;

        rows.iter()
            .map(|row| {
                Ok(TagMatch {
                    key: row.try_get("tag_key")?,
                    value: row.try_get("tag_value")?,
                })
            })
            .collect()
    }
}

#[async_trait]
impl TagMuteRuleRepository for SqliteTagMuteRuleRepository {
    async fn create(&self, new: NewTagMuteRule) -> Result<TagMuteRule> {
        let id = Id::new_v4();
        let now = chrono::Utc::now();
        let now_db = ts_to_db(now);

        // A write transaction (`BEGIN IMMEDIATE`), so the rule + its tags land
        // atomically without risking the deferred-upgrade `SQLITE_BUSY`.
        let mut tx = begin_write(&self.db).await?;

        sqlx::query(
            "INSERT INTO tag_mute_rules (id, project_id, name, created_by, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id_to_db(id))
        .bind(id_to_db(new.project_id))
        .bind(new.name.as_deref())
        .bind(new.created_by.map(id_to_db))
        .bind(&now_db)
        .bind(&now_db)
        .execute(&mut *tx)
        .await?;

        for tag in &new.tags {
            sqlx::query(
                "INSERT INTO tag_mute_rule_tags (rule_id, tag_key, tag_value) VALUES (?, ?, ?)",
            )
            .bind(id_to_db(id))
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
        let rows = sqlx::query(
            "SELECT id, project_id, name, created_by, created_at, updated_at \
             FROM tag_mute_rules WHERE project_id = ? ORDER BY created_at DESC, id",
        )
        .bind(id_to_db(project_id))
        .fetch_all(&self.db)
        .await?;

        let mut rules = Vec::with_capacity(rows.len());
        for row in rows {
            let id = id_from_db(&row.try_get::<String, _>("id")?)?;
            let created_by = row
                .try_get::<Option<String>, _>("created_by")?
                .map(|s| id_from_db(&s))
                .transpose()?;
            rules.push(TagMuteRule {
                id,
                project_id: id_from_db(&row.try_get::<String, _>("project_id")?)?,
                name: row.try_get("name")?,
                tags: self.tags_for(id).await?,
                created_by,
                created_at: ts_from_db(&row.try_get::<String, _>("created_at")?)?,
                updated_at: ts_from_db(&row.try_get::<String, _>("updated_at")?)?,
            });
        }
        Ok(rules)
    }

    async fn delete(&self, project_id: Id, rule_id: Id) -> Result<bool> {
        // Scope the delete to the project so a rule from another project cannot
        // be removed by id. Child tags cascade (ON DELETE CASCADE).
        let result = sqlx::query("DELETE FROM tag_mute_rules WHERE id = ? AND project_id = ?")
            .bind(id_to_db(rule_id))
            .bind(id_to_db(project_id))
            .execute(&self.db)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
