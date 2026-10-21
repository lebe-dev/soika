//! SQLite [`ProjectRepository`] adapter.
//!
//! Persistence notes (mirroring `migrations/0001_init.sql`):
//!   * `id` / `team_id` are stored as TEXT (UUID v4 string form).
//!   * Timestamps are TEXT in RFC3339/ISO-8601 (UTC).
//!   * `muted` is INTEGER 0/1.
//!   * `webhook_url` is TEXT and nullable (NULL = no webhook channel).
//!
//! The SQLite-specific bit is the runtime row decoding of TEXT columns, which
//! is isolated in [`row_to_project`].

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
const PROJECT_COLS: &str = "id, short_id, team_id, name, slug, dsn_public_key, \
     retention_events, retention_days, muted, webhook_url, created_at, updated_at";

/// Map a row (selected via [`PROJECT_COLS`]) into a domain [`Project`].
///
/// SQLite-specific: TEXT id/timestamps are parsed here so the rest of the code
/// stays in domain types.
fn row_to_project(row: &sqlx::sqlite::SqliteRow) -> Result<Project> {
    Ok(Project {
        id: parse_id(row.try_get::<String, _>("id")?)?,
        short_id: row.try_get("short_id")?,
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
        let short_id = super::unique_short_id(&self.db, "projects").await?;
        let now = chrono::Utc::now();
        let sql = format!(
            "INSERT INTO projects \
                (id, short_id, team_id, name, slug, dsn_public_key, retention_events, retention_days, muted, webhook_url, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?) \
             RETURNING {PROJECT_COLS}"
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(id.to_string())
            .bind(short_id)
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
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(id.to_string())
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    async fn find_by_short_id(&self, short_id: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE short_id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(short_id)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE slug = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(slug)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    async fn find_by_dsn(&self, dsn_public_key: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE dsn_public_key = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(dsn_public_key)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_project).transpose()
    }

    async fn list(&self) -> Result<Vec<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects ORDER BY name");
        let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .fetch_all(&self.db)
            .await?;
        rows.iter().map(row_to_project).collect()
    }

    async fn list_for_team(&self, team_id: Id) -> Result<Vec<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE team_id = ? ORDER BY name");
        let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(team_id.to_string())
            .fetch_all(&self.db)
            .await?;
        rows.iter().map(row_to_project).collect()
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Project>> {
        // Variant A (closed membership): a user can see a project iff they are a
        // member of the team that owns it. There is no direct project membership
        // any more (the `memberships` table was dropped by migration 0013).
        // Select `p.*` rather than the bare `PROJECT_COLS` list: the joined table
        // also has `created_at`, so an unqualified projection would be ambiguous.
        // `row_to_project` reads columns by name, so `p.*` maps cleanly.
        let sql = "SELECT p.* FROM projects p \
             JOIN team_members tm ON tm.team_id = p.team_id \
             WHERE tm.user_id = ? \
             ORDER BY name";
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
                team_id = COALESCE(?, team_id), \
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
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(update.name)
            .bind(update.team_id.map(|id| id.to_string()))
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
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
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
