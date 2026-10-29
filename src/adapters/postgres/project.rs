//! PostgreSQL [`ProjectRepository`] adapter.

use super::{Db, is_unique_violation, now, unique_short_id};
use crate::domain::{Id, Project, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{NewProject, ProjectRepository, ProjectUpdate};
use async_trait::async_trait;
use sqlx::{AssertSqlSafe, FromRow};

#[derive(Clone)]
pub struct PgProjectRepository {
    db: Db,
}

impl PgProjectRepository {
    pub fn new(db: Db) -> Self {
        PgProjectRepository { db }
    }
}

/// Columns selected for a `Project`, in a fixed order shared by every query.
const PROJECT_COLS: &str = "id, short_id, team_id, name, slug, dsn_public_key, \
     retention_events, retention_days, muted, webhook_url, created_at, updated_at, orphaned_at";

#[derive(FromRow)]
struct ProjectRow {
    id: Id,
    short_id: String,
    team_id: Id,
    name: String,
    slug: String,
    dsn_public_key: String,
    retention_events: i64,
    retention_days: i64,
    muted: bool,
    webhook_url: Option<String>,
    created_at: Timestamp,
    updated_at: Timestamp,
    orphaned_at: Option<Timestamp>,
}

impl From<ProjectRow> for Project {
    fn from(row: ProjectRow) -> Self {
        Project {
            orphaned_at: row.orphaned_at,
            id: row.id,
            short_id: row.short_id,
            team_id: row.team_id,
            name: row.name,
            slug: row.slug,
            dsn_public_key: row.dsn_public_key,
            retention_events: row.retention_events,
            retention_days: row.retention_days,
            muted: row.muted,
            webhook_url: row.webhook_url,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl PgProjectRepository {
    /// Fetch at most one project matching `column = value`.
    async fn find_one<T>(&self, column: &str, value: T) -> Result<Option<Project>>
    where
        T: for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send,
    {
        let row = sqlx::query_as::<_, ProjectRow>(AssertSqlSafe(format!(
            "SELECT {PROJECT_COLS} FROM projects WHERE {column} = $1"
        )))
        .bind(value)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(Project::from))
    }
}

#[async_trait]
impl ProjectRepository for PgProjectRepository {
    async fn create(&self, new: NewProject) -> Result<Project> {
        let short_id = unique_short_id(&self.db, "projects").await?;
        let now = now();
        let row = sqlx::query_as::<_, ProjectRow>(AssertSqlSafe(format!(
            "INSERT INTO projects \
                (id, short_id, team_id, name, slug, dsn_public_key, retention_events, \
                 retention_days, muted, webhook_url, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, FALSE, $9, $10, $10) \
             RETURNING {PROJECT_COLS}"
        )))
        .bind(Id::new_v4())
        .bind(short_id)
        .bind(new.team_id)
        .bind(new.name)
        .bind(new.slug)
        .bind(new.dsn_public_key)
        .bind(new.retention_events)
        .bind(new.retention_days)
        .bind(new.webhook_url)
        .bind(now)
        .fetch_one(&self.db)
        .await
        .map_err(map_conflict)?;
        Ok(row.into())
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Project>> {
        self.find_one("id", id).await
    }

    async fn find_by_short_id(&self, short_id: &str) -> Result<Option<Project>> {
        self.find_one("short_id", short_id).await
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        self.find_one("slug", slug).await
    }

    async fn find_by_dsn(&self, dsn_public_key: &str) -> Result<Option<Project>> {
        self.find_one("dsn_public_key", dsn_public_key).await
    }

    async fn list(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query_as::<_, ProjectRow>(AssertSqlSafe(format!(
            "SELECT {PROJECT_COLS} FROM projects ORDER BY name"
        )))
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(Project::from).collect())
    }

    async fn list_for_team(&self, team_id: Id) -> Result<Vec<Project>> {
        let rows = sqlx::query_as::<_, ProjectRow>(AssertSqlSafe(format!(
            "SELECT {PROJECT_COLS} FROM projects WHERE team_id = $1 ORDER BY name"
        )))
        .bind(team_id)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(Project::from).collect())
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Project>> {
        // Closed membership: a user sees a project iff they are a member of the
        // team that owns it.
        let rows = sqlx::query_as::<_, ProjectRow>(
            "SELECT p.id, p.short_id, p.team_id, p.name, p.slug, p.dsn_public_key, \
                    p.retention_events, p.retention_days, p.muted, p.webhook_url, \
                    p.created_at, p.updated_at, p.orphaned_at \
             FROM projects p \
             JOIN team_members tm ON tm.team_id = p.team_id \
             WHERE tm.user_id = $1 \
             ORDER BY p.name",
        )
        .bind(user_id)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(Project::from).collect())
    }

    async fn update(&self, id: Id, update: ProjectUpdate) -> Result<Project> {
        // COALESCE keeps every field the caller left as `None`. `webhook_url` is
        // the exception: `Some(None)` must clear it to NULL, which COALESCE
        // cannot tell apart from "leave", so an explicit "present" flag drives
        // a CASE instead.
        let row = sqlx::query_as::<_, ProjectRow>(AssertSqlSafe(format!(
            "UPDATE projects SET \
                name = COALESCE($1, name), \
                team_id = COALESCE($2, team_id), \
                retention_events = COALESCE($3, retention_events), \
                retention_days = COALESCE($4, retention_days), \
                muted = COALESCE($5, muted), \
                webhook_url = CASE WHEN $6 THEN $7 ELSE webhook_url END, \
                updated_at = $8 \
             WHERE id = $9 \
             RETURNING {PROJECT_COLS}"
        )))
        .bind(update.name)
        .bind(update.team_id)
        .bind(update.retention_events)
        .bind(update.retention_days)
        .bind(update.muted)
        .bind(update.webhook_url.is_some())
        .bind(update.webhook_url.flatten())
        .bind(now())
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        row.map(Project::from)
            .ok_or_else(|| Error::not_found(format!("project {id}")))
    }

    async fn regenerate_dsn(&self, id: Id, new_dsn_public_key: String) -> Result<Project> {
        let row = sqlx::query_as::<_, ProjectRow>(AssertSqlSafe(format!(
            "UPDATE projects SET dsn_public_key = $1, updated_at = $2 \
             WHERE id = $3 RETURNING {PROJECT_COLS}"
        )))
        .bind(new_dsn_public_key)
        .bind(now())
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .map_err(map_conflict)?;
        row.map(Project::from)
            .ok_or_else(|| Error::not_found(format!("project {id}")))
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }
}

/// Map a UNIQUE-constraint violation onto a domain [`Error::Conflict`].
fn map_conflict(e: sqlx::Error) -> Error {
    if is_unique_violation(&e) {
        return Error::Conflict("project slug or DSN already exists".into());
    }
    Error::Db(e)
}
