//! SQLite adapters implementing every repository port.
//!
//! SQL is kept ANSI-friendly so a PostgreSQL backend can be added later
//!. The SQLite-specific representation choices are isolated in the
//! small conversion helpers below:
//!
//!   * UUID ids are stored as TEXT (lowercase hyphenated form) — see
//!     [`id_to_db`] / [`id_from_db`].
//!   * Timestamps are stored as TEXT in RFC3339/ISO-8601 (UTC) — see
//!     [`ts_to_db`] / [`ts_from_db`].
//!   * Booleans are stored as INTEGER 0/1 — see [`bool_to_db`].
//!
//! Queries use runtime-checked `sqlx::query*` with explicit `FromRow` row
//! structs (rather than the compile-time `query!` macros) so the crate builds
//! without a live database or an offline metadata cache. The integration step
//! can opt into compile-time checking by running `cargo sqlx prepare` once the
//! schema is migrated.

mod event;
mod invite;
mod issue;
mod membership;
mod project;
mod session;
mod settings;
mod team;
mod user;

pub use event::SqliteEventRepository;
pub use invite::SqliteInviteRepository;
pub use issue::SqliteIssueRepository;
pub use membership::SqliteMembershipRepository;
pub use project::SqliteProjectRepository;
pub use session::SqliteSessionRepository;
pub use settings::SqliteSettingsRepository;
pub use team::SqliteTeamRepository;
pub use user::SqliteUserRepository;

use crate::domain::{Id, Timestamp};
use crate::error::Error;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

/// Shared handle held by each adapter struct.
pub type Db = SqlitePool;

/// Serialize an [`Id`] (UUID) to its TEXT database form.
pub(crate) fn id_to_db(id: Id) -> String {
    id.to_string()
}

/// Parse an [`Id`] (UUID) from its TEXT database form.
pub(crate) fn id_from_db(s: &str) -> Result<Id, Error> {
    Id::parse_str(s).map_err(|e| Error::internal(format!("invalid uuid in db: {s}: {e}")))
}

/// Serialize a [`Timestamp`] to its TEXT database form (RFC3339, UTC).
pub(crate) fn ts_to_db(ts: Timestamp) -> String {
    ts.to_rfc3339()
}

/// Parse a [`Timestamp`] from its TEXT database form (RFC3339, UTC).
pub(crate) fn ts_from_db(s: &str) -> Result<Timestamp, Error> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| Error::internal(format!("invalid timestamp in db: {s}: {e}")))
}

/// Map an SQLite integer boolean (0/1) to a Rust `bool`.
pub(crate) fn bool_from_db(v: i64) -> bool {
    v != 0
}

/// Map a Rust `bool` to its SQLite integer form (0/1).
pub(crate) fn bool_to_db(v: bool) -> i64 {
    i64::from(v)
}

/// Inspect an `INSERT`/`UPDATE` error and classify a UNIQUE-constraint
/// violation as a domain [`Error::Conflict`]. SQLite surfaces these as a
/// `Database` error whose message mentions "UNIQUE constraint failed"; this
/// check is the only SQLite-dialect-aware bit and is isolated here.
pub(crate) fn conflict_or_db(err: sqlx::Error, what: impl Into<String>) -> Error {
    if let sqlx::Error::Database(db_err) = &err {
        let msg = db_err.message().to_ascii_lowercase();
        if msg.contains("unique constraint") || msg.contains("constraint failed") {
            return Error::Conflict(what.into());
        }
    }
    Error::Db(err)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::Db;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Build a fresh in-memory SQLite pool with the 0001 schema applied.
    ///
    /// `:memory:` databases are per-connection, so the pool is pinned to a
    /// single connection to keep all queries on the same in-memory database.
    pub(crate) async fn test_pool() -> Db {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");

        // Enforce FK constraints (SQLite leaves them off by default).
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("enable foreign keys");

        crate::MIGRATOR.run(&pool).await.expect("run migrations");

        pool
    }

    /// Insert a team row directly (bypassing the team adapter) and return its id.
    pub(crate) async fn insert_team(pool: &Db, name: &str) -> crate::domain::Id {
        let id = crate::domain::Id::new_v4();
        let now = super::ts_to_db(chrono::Utc::now());
        sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(super::id_to_db(id))
            .bind(name)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .expect("insert team");
        id
    }

    /// Insert a project row directly (bypassing the project adapter) and return its id.
    pub(crate) async fn insert_project(
        pool: &Db,
        team_id: crate::domain::Id,
        slug: &str,
        dsn: &str,
    ) -> crate::domain::Id {
        let id = crate::domain::Id::new_v4();
        let now = super::ts_to_db(chrono::Utc::now());
        sqlx::query(
            "INSERT INTO projects (id, team_id, name, slug, dsn_public_key, retention_events, \
             muted, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 1000, 0, ?, ?)",
        )
        .bind(super::id_to_db(id))
        .bind(super::id_to_db(team_id))
        .bind(slug)
        .bind(slug)
        .bind(dsn)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("insert project");
        id
    }

    /// Insert a user row directly and return its id.
    pub(crate) async fn insert_user(pool: &Db, email: &str) -> crate::domain::Id {
        let id = crate::domain::Id::new_v4();
        let now = super::ts_to_db(chrono::Utc::now());
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, is_admin, \
             notifications_enabled, created_at, updated_at) VALUES (?, ?, ?, 'hash', 0, 1, ?, ?)",
        )
        .bind(super::id_to_db(id))
        .bind(email)
        .bind(email)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("insert user");
        id
    }
}
