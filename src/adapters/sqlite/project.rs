//! SQLite [`ProjectRepository`] adapter.
//!
//! Persistence notes (mirroring `migrations/0001_init.sql`):
//!   * `id` / `team_id` are stored as TEXT (UUID v4 string form).
//!   * Timestamps are TEXT in RFC3339/ISO-8601 (UTC).
//!   * `muted` is INTEGER 0/1.
//!   * `webhook_url` is TEXT and nullable (NULL = no webhook channel).
//!
//! SQL is kept ANSI-friendly so a Postgres backend can drop in later;
//! the only SQLite-specific bit is the runtime row decoding of TEXT columns,
//! which is isolated in [`row_to_project`].

use super::Db;
use crate::domain::{Id, Project};
use crate::error::{Error, Result};
use crate::ports::{NewProject, ProjectRepository, ProjectUpdate};
use async_trait::async_trait;
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteProjectRepository {
    db: Db,
}

impl SqliteProjectRepository {
    pub fn new(db: Db) -> Self {
        SqliteProjectRepository { db }
    }
}

/// Columns selected for a `Project`, in a fixed order shared by every query.
const PROJECT_COLS: &str = "id, team_id, name, slug, dsn_public_key, \
     retention_events, retention_days, muted, webhook_url, created_at, updated_at";

/// Map a row (selected via [`PROJECT_COLS`]) into a domain [`Project`].
///
/// SQLite-specific: TEXT id/timestamps are parsed here so the rest of the code
/// stays in domain types.
fn row_to_project(row: &sqlx::sqlite::SqliteRow) -> Result<Project> {
    Ok(Project {
        id: parse_id(row.try_get::<String, _>("id")?)?,
        team_id: parse_id(row.try_get::<String, _>("team_id")?)?,
        name: row.try_get("name")?,
        slug: row.try_get("slug")?,
        dsn_public_key: row.try_get("dsn_public_key")?,
        retention_events: row.try_get("retention_events")?,
        retention_days: row.try_get("retention_days")?,
        muted: row.try_get::<i64, _>("muted")? != 0,
        webhook_url: row.try_get("webhook_url")?,
        created_at: parse_ts(row.try_get::<String, _>("created_at")?)?,
        updated_at: parse_ts(row.try_get::<String, _>("updated_at")?)?,
    })
}

fn parse_id(s: String) -> Result<Id> {
    Id::parse_str(&s).map_err(|e| Error::internal(format!("invalid uuid in db: {e}")))
}

fn parse_ts(s: String) -> Result<crate::domain::Timestamp> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| Error::internal(format!("invalid timestamp in db: {e}")))
}

#[async_trait]
impl ProjectRepository for SqliteProjectRepository {
    async fn create(&self, new: NewProject) -> Result<Project> {
        let id = Id::new_v4();
        let now = chrono::Utc::now();
        let sql = format!(
            "INSERT INTO projects \
                (id, team_id, name, slug, dsn_public_key, retention_events, retention_days, muted, webhook_url, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?) \
             RETURNING {PROJECT_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .bind(new.team_id.to_string())
            .bind(new.name)
            .bind(new.slug)
            .bind(new.dsn_public_key)
            .bind(new.retention_events)
            .bind(new.retention_days)
            .bind(new.webhook_url)
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .fetch_one(&self.db)
            .await
            .map_err(map_conflict)?;
        row_to_project(&row)
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE slug = ?");
        let row = sqlx::query(&sql)
            .bind(slug)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    async fn find_by_dsn(&self, dsn_public_key: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE dsn_public_key = ?");
        let row = sqlx::query(&sql)
            .bind(dsn_public_key)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    async fn list(&self) -> Result<Vec<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects ORDER BY name");
        let rows = sqlx::query(&sql).fetch_all(&self.db).await?;
        rows.iter().map(row_to_project).collect()
    }

    async fn list_for_team(&self, team_id: Id) -> Result<Vec<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE team_id = ? ORDER BY name");
        let rows = sqlx::query(&sql)
            .bind(team_id.to_string())
            .fetch_all(&self.db)
            .await?;
        rows.iter().map(row_to_project).collect()
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Project>> {
        // Projects the user can see via a project membership. Select `p.*`
        // rather than the bare `PROJECT_COLS` list: `memberships` also has
        // `created_at`/`updated_at`, so an unqualified projection over the join
        // is ambiguous. `row_to_project` reads columns by name, so `p.*` (the
        // project columns, unqualified in the result set) maps cleanly.
        let sql = "SELECT p.* FROM projects p \
             JOIN memberships m ON m.project_id = p.id \
             WHERE m.user_id = ? \
             ORDER BY p.name";
        let rows = sqlx::query(sql)
            .bind(user_id.to_string())
            .fetch_all(&self.db)
            .await?;
        rows.iter().map(row_to_project).collect()
    }

    async fn update(&self, id: Id, update: ProjectUpdate) -> Result<Project> {
        // Build a dynamic SET clause from the provided fields. COALESCE-style
        // partial update keeps the query ANSI-friendly without per-field SQL.
        //
        // `webhook_url` is the exception: it must support all three
        // double-`Option` intents — `None` leaves it unchanged, `Some(None)`
        // clears it to NULL, and `Some(Some(url))` sets it. COALESCE cannot
        // express clear-to-NULL (a NULL bind is indistinguishable from "leave"),
        // so a CASE driven by an explicit "is this field present?" flag is used
        // instead: when the flag is 0 (`None`) the old value is kept; when it is
        // 1 the bound value (which may itself be NULL) is written.
        let now = chrono::Utc::now();
        let sql = format!(
            "UPDATE projects SET \
                name = COALESCE(?, name), \
                retention_events = COALESCE(?, retention_events), \
                retention_days = COALESCE(?, retention_days), \
                muted = COALESCE(?, muted), \
                webhook_url = CASE WHEN ? = 1 THEN ? ELSE webhook_url END, \
                updated_at = ? \
             WHERE id = ? \
             RETURNING {PROJECT_COLS}"
        );
        let webhook_present = i64::from(update.webhook_url.is_some());
        let webhook_value = update.webhook_url.flatten();
        let row = sqlx::query(&sql)
            .bind(update.name)
            .bind(update.retention_events)
            .bind(update.retention_days)
            .bind(update.muted.map(|m| if m { 1_i64 } else { 0 }))
            .bind(webhook_present)
            .bind(webhook_value)
            .bind(now.to_rfc3339())
            .bind(id.to_string())
            .fetch_optional(&self.db)
            .await?;
        match row {
            Some(r) => row_to_project(&r),
            None => Err(Error::not_found(format!("project {id}"))),
        }
    }

    async fn regenerate_dsn(&self, id: Id, new_dsn_public_key: String) -> Result<Project> {
        let now = chrono::Utc::now();
        let sql = format!(
            "UPDATE projects SET dsn_public_key = ?, updated_at = ? \
             WHERE id = ? RETURNING {PROJECT_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(new_dsn_public_key)
            .bind(now.to_rfc3339())
            .bind(id.to_string())
            .fetch_optional(&self.db)
            .await
            .map_err(map_conflict)?;
        match row {
            Some(r) => row_to_project(&r),
            None => Err(Error::not_found(format!("project {id}"))),
        }
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.db)
            .await?;
        Ok(())
    }
}

/// Map a UNIQUE-constraint violation onto a domain [`Error::Conflict`].
fn map_conflict(e: sqlx::Error) -> Error {
    if let sqlx::Error::Database(ref db) = e
        && db.is_unique_violation()
    {
        return Error::Conflict("project slug or DSN already exists".into());
    }
    Error::Db(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> Db {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        pool
    }

    async fn seed_team(pool: &Db) -> Id {
        let team_id = Id::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(team_id.to_string())
            .bind("team-a")
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .unwrap();
        team_id
    }

    fn new_project(team_id: Id, slug: &str, dsn: &str) -> NewProject {
        NewProject {
            team_id,
            name: format!("Project {slug}"),
            slug: slug.to_string(),
            dsn_public_key: dsn.to_string(),
            retention_events: 1000,
            retention_days: 0,
            webhook_url: None,
        }
    }

    #[tokio::test]
    async fn create_and_find_by_dsn() {
        let pool = pool().await;
        let team_id = seed_team(&pool).await;
        let repo = SqliteProjectRepository::new(pool);

        let created = repo
            .create(new_project(team_id, "alpha", "dsn-key-alpha"))
            .await
            .unwrap();
        assert_eq!(created.team_id, team_id);
        assert!(!created.muted);

        let found = repo.find_by_dsn("dsn-key-alpha").await.unwrap().unwrap();
        assert_eq!(found.id, created.id);
        assert!(repo.find_by_dsn("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_for_user_returns_member_projects() {
        // Regression: `memberships` also has a `created_at`, so projecting the
        // bare column list over the join was ambiguous and errored for every
        // non-admin caller. The join must qualify its projection.
        let pool = pool().await;
        let team_id = seed_team(&pool).await;
        let repo = SqliteProjectRepository::new(pool.clone());

        let project = repo
            .create(new_project(team_id, "alpha", "dsn-alpha"))
            .await
            .unwrap();

        let user_id = Id::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, created_at, updated_at) \
             VALUES (?, 'm@example.com', 'Member', 'x', ?, ?)",
        )
        .bind(user_id.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (project_id, user_id, role, created_at) \
             VALUES (?, ?, 'member', ?)",
        )
        .bind(project.id.to_string())
        .bind(user_id.to_string())
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let mine = repo.list_for_user(user_id).await.unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].id, project.id);

        // A user with no memberships sees nothing.
        let stranger = repo.list_for_user(Id::new_v4()).await.unwrap();
        assert!(stranger.is_empty());
    }

    #[tokio::test]
    async fn duplicate_dsn_is_conflict() {
        let pool = pool().await;
        let team_id = seed_team(&pool).await;
        let repo = SqliteProjectRepository::new(pool);

        repo.create(new_project(team_id, "a", "same-dsn"))
            .await
            .unwrap();
        let err = repo
            .create(new_project(team_id, "b", "same-dsn"))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Conflict(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn partial_update_leaves_other_fields() {
        let pool = pool().await;
        let team_id = seed_team(&pool).await;
        let repo = SqliteProjectRepository::new(pool);

        let p = repo
            .create(new_project(team_id, "alpha", "dsn-alpha"))
            .await
            .unwrap();

        let updated = repo
            .update(
                p.id,
                ProjectUpdate {
                    muted: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(updated.muted);
        assert_eq!(updated.name, p.name, "name preserved by COALESCE update");
        assert_eq!(updated.retention_events, 1000);
        assert_eq!(updated.retention_days, 0, "retention_days untouched");
    }

    #[tokio::test]
    async fn webhook_url_round_trips_through_create_and_partial_update() {
        let pool = pool().await;
        let team_id = seed_team(&pool).await;
        let repo = SqliteProjectRepository::new(pool);

        // Create without a webhook URL: NULL round-trips to None.
        let without = repo
            .create(new_project(team_id, "no-hook", "dsn-no-hook"))
            .await
            .unwrap();
        assert_eq!(without.webhook_url, None);
        let reread = repo.find_by_id(without.id).await.unwrap().unwrap();
        assert_eq!(reread.webhook_url, None);

        // Create WITH a webhook URL: it survives the round-trip.
        let mut new = new_project(team_id, "hook", "dsn-hook");
        new.webhook_url = Some("https://hook.example/x".into());
        let with = repo.create(new).await.unwrap();
        assert_eq!(with.webhook_url.as_deref(), Some("https://hook.example/x"));
        let reread = repo.find_by_id(with.id).await.unwrap().unwrap();
        assert_eq!(
            reread.webhook_url.as_deref(),
            Some("https://hook.example/x")
        );

        // Some(Some(url)) sets the webhook URL on an existing project.
        let set = repo
            .update(
                without.id,
                ProjectUpdate {
                    webhook_url: Some(Some("https://h".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(set.webhook_url.as_deref(), Some("https://h"));

        // None (e.g. a muted-only update) leaves the existing webhook unchanged.
        let unchanged = repo
            .update(
                without.id,
                ProjectUpdate {
                    muted: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(unchanged.muted);
        assert_eq!(
            unchanged.webhook_url.as_deref(),
            Some("https://h"),
            "None leaves webhook_url unchanged"
        );

        // Some(None) clears the webhook to NULL (disable the channel). This must
        // actually persist NULL, not silently keep the old value.
        let cleared = repo
            .update(
                without.id,
                ProjectUpdate {
                    webhook_url: Some(None),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            cleared.webhook_url, None,
            "Some(None) clears webhook_url to NULL"
        );
        let reread = repo.find_by_id(without.id).await.unwrap().unwrap();
        assert_eq!(reread.webhook_url, None, "cleared NULL is persisted");
        assert!(
            reread.muted,
            "clearing webhook_url leaves other fields (muted) intact"
        );
    }

    #[tokio::test]
    async fn retention_days_round_trips_through_create_and_update() {
        let pool = pool().await;
        let team_id = seed_team(&pool).await;
        let repo = SqliteProjectRepository::new(pool);

        // Create with an explicit age-based retention window.
        let mut new = new_project(team_id, "beta", "dsn-beta");
        new.retention_days = 7;
        let created = repo.create(new).await.unwrap();
        assert_eq!(created.retention_days, 7, "created value persisted");

        // Some(value) updates the column.
        let updated = repo
            .update(
                created.id,
                ProjectUpdate {
                    retention_days: Some(30),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.retention_days, 30, "Some(value) updates the column");

        // None leaves the column unchanged.
        let unchanged = repo
            .update(
                created.id,
                ProjectUpdate {
                    name: Some("Renamed".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            unchanged.retention_days, 30,
            "None leaves retention_days unchanged"
        );
        assert_eq!(unchanged.name, "Renamed");
    }
}
