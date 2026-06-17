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
mod favorite;
mod invite;
mod issue;
mod mute_rule;
mod project;
mod session;
mod settings;
mod team;
mod user;

pub use event::SqliteEventRepository;
pub use favorite::SqliteFavoriteRepository;
pub use invite::SqliteInviteRepository;
pub use issue::SqliteIssueRepository;
pub use mute_rule::SqliteTagMuteRuleRepository;
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

/// Alphabet for short public ids — lowercase alphanumerics, URL-friendly and
/// unambiguous in a browser address bar (36^6 ≈ 2.1B combinations at length 6).
const SHORT_ID_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Length of a generated short id. Kept small for compact URLs.
const SHORT_ID_LEN: usize = 6;

/// Generate a random short public id (see [`SHORT_ID_ALPHABET`]).
///
/// Callers persist this into a UNIQUE column; the astronomically rare collision
/// is handled by retrying (see [`unique_short_id`]).
pub(crate) fn generate_short_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..SHORT_ID_LEN)
        .map(|_| SHORT_ID_ALPHABET[rng.gen_range(0..SHORT_ID_ALPHABET.len())] as char)
        .collect()
}

/// Assign a unique random short id to every project/issue row still missing one.
///
/// Runs once at startup, right after migrations: migration 0007 adds the
/// `short_id` column (leaving existing rows NULL) and the backfill cannot live in
/// SQL because a random value there could collide and abort the UNIQUE-index
/// build. Here each NULL row gets a code from [`unique_short_id`], which retries
/// until it finds a free one — random *and* guaranteed unique. A no-op on a fresh
/// database (and on every subsequent boot once all rows are filled).
pub async fn backfill_short_ids(db: &Db) -> Result<(), Error> {
    for table in ["projects", "issues"] {
        let select = format!("SELECT id FROM {table} WHERE short_id IS NULL LIMIT 1");
        let update = format!("UPDATE {table} SET short_id = ? WHERE id = ?");
        while let Some(row) = sqlx::query(&select).fetch_optional(db).await? {
            let id: String = sqlx::Row::try_get(&row, "id")?;
            let code = unique_short_id(db, table).await?;
            sqlx::query(&update).bind(code).bind(id).execute(db).await?;
        }
    }
    Ok(())
}

/// Generate a short id not already present in `table.short_id`.
///
/// Pre-checks for a free code (rather than relying on the UNIQUE constraint and
/// distinguishing it from the slug/DSN constraints on the same INSERT). The loop
/// is bounded; with a 2.1-billion keyspace it effectively never iterates twice.
pub(crate) async fn unique_short_id(db: &Db, table: &str) -> Result<String, Error> {
    let sql = format!("SELECT 1 FROM {table} WHERE short_id = ? LIMIT 1");
    for _ in 0..16 {
        let candidate = generate_short_id();
        let taken = sqlx::query(&sql)
            .bind(&candidate)
            .fetch_optional(db)
            .await?
            .is_some();
        if !taken {
            return Ok(candidate);
        }
    }
    Err(Error::internal("could not allocate a unique short id"))
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
            "INSERT INTO projects (id, short_id, team_id, name, slug, dsn_public_key, \
             retention_events, muted, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 1000, 0, ?, ?)",
        )
        .bind(super::id_to_db(id))
        .bind(super::generate_short_id())
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

    #[tokio::test]
    async fn backfill_short_ids_fills_null_rows_with_unique_codes() {
        let pool = test_pool().await;
        let team_id = insert_team(&pool, "t").await;

        // Two projects with NULL short_id (mimicking rows created before
        // migration 0007 added the column).
        let now = super::ts_to_db(chrono::Utc::now());
        for (n, slug) in [("a", "sa"), ("b", "sb")] {
            sqlx::query(
                "INSERT INTO projects (id, team_id, name, slug, dsn_public_key, \
                 retention_events, muted, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 1000, 0, ?, ?)",
            )
            .bind(super::id_to_db(crate::domain::Id::new_v4()))
            .bind(super::id_to_db(team_id))
            .bind(n)
            .bind(slug)
            .bind(format!("dsn-{slug}"))
            .bind(&now)
            .bind(&now)
            .execute(&pool)
            .await
            .expect("insert null-short_id project");
        }

        super::backfill_short_ids(&pool).await.expect("backfill");

        let codes: Vec<String> = sqlx::query("SELECT short_id FROM projects")
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|r| sqlx::Row::try_get::<String, _>(r, "short_id").unwrap())
            .collect();
        assert_eq!(codes.len(), 2);
        assert!(codes.iter().all(|c| c.len() == 6));
        assert_ne!(codes[0], codes[1], "backfilled codes are unique");

        // Idempotent: a second run leaves the now-filled rows untouched.
        super::backfill_short_ids(&pool)
            .await
            .expect("second backfill");
        let after: Vec<String> = sqlx::query("SELECT short_id FROM projects ORDER BY short_id")
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|r| sqlx::Row::try_get::<String, _>(r, "short_id").unwrap())
            .collect();
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(after, sorted, "second run is a no-op");
    }

    /// Insert a user row directly and return its id.
    ///
    /// Leaves `instance_role` at its column default (`'member'`).
    pub(crate) async fn insert_user(pool: &Db, email: &str) -> crate::domain::Id {
        let id = crate::domain::Id::new_v4();
        let now = super::ts_to_db(chrono::Utc::now());
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, \
             notifications_enabled, created_at, updated_at) VALUES (?, ?, ?, 'hash', 1, ?, ?)",
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
