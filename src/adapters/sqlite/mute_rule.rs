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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::{insert_project, insert_team, insert_user, test_pool};

    fn tag(k: &str, v: &str) -> TagMatch {
        TagMatch {
            key: k.to_string(),
            value: v.to_string(),
        }
    }

    #[tokio::test]
    async fn create_list_and_delete_roundtrip() {
        let pool = test_pool().await;
        let team = insert_team(&pool, "t").await;
        let project = insert_project(&pool, team, "p", "dsn-p").await;
        let user = insert_user(&pool, "u@example.com").await;
        let repo = SqliteTagMuteRuleRepository::new(pool);

        let created = repo
            .create(NewTagMuteRule {
                project_id: project,
                name: Some("staging noise".into()),
                created_by: Some(user),
                tags: vec![tag("environment", "staging"), tag("server", "ci")],
            })
            .await
            .expect("create");

        assert_eq!(created.tags.len(), 2);

        let listed = repo.list_for_project(project).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
        assert_eq!(listed[0].name.as_deref(), Some("staging noise"));
        assert_eq!(listed[0].created_by, Some(user));
        assert_eq!(listed[0].tags.len(), 2);

        // Wrong project id does not delete.
        assert!(
            !repo
                .delete(Id::new_v4(), created.id)
                .await
                .expect("delete other")
        );
        assert!(repo.delete(project, created.id).await.expect("delete"));
        assert!(
            repo.list_for_project(project)
                .await
                .expect("list")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn deleting_project_cascades_rules() {
        let pool = test_pool().await;
        let team = insert_team(&pool, "t").await;
        let project = insert_project(&pool, team, "p", "dsn-p").await;
        let repo = SqliteTagMuteRuleRepository::new(pool.clone());

        repo.create(NewTagMuteRule {
            project_id: project,
            name: None,
            created_by: None,
            tags: vec![tag("environment", "staging")],
        })
        .await
        .expect("create");

        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id_to_db(project))
            .execute(&pool)
            .await
            .expect("delete project");

        let orphan_tags: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tag_mute_rule_tags")
            .fetch_one(&pool)
            .await
            .expect("count")
            .try_get("n")
            .unwrap();
        assert_eq!(orphan_tags, 0, "tag rows cascade when the project is gone");
    }
}
