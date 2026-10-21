//! Database backend selection, connection pools and migrations.
//!
//! soika runs on SQLite (one file, the default) or PostgreSQL. The backend is
//! picked from the `DATABASE_URL` scheme ([`DbBackend::from_url`]); both drivers
//! are always compiled in. This module is the single place connection and
//! locking behaviour is configured — adapters receive a ready pool.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{PgPool, SqlitePool};

use crate::config;
use crate::error::{Error, Result};

/// Embedded SQLite migrations (schema), run at startup.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/sqlite");

/// Embedded PostgreSQL migrations (schema), run at startup.
pub static PG_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/postgres");

/// Which database engine a `DATABASE_URL` points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbBackend {
    Sqlite,
    Postgres,
}

impl DbBackend {
    /// Detect the backend from the URL scheme: `sqlite:` or
    /// `postgres:` / `postgresql:`. Anything else is a configuration error.
    pub fn from_url(database_url: &str) -> Result<Self> {
        let scheme = database_url
            .split_once(':')
            .map(|(scheme, _)| scheme.to_ascii_lowercase())
            .unwrap_or_default();
        match scheme.as_str() {
            "sqlite" => Ok(DbBackend::Sqlite),
            "postgres" | "postgresql" => Ok(DbBackend::Postgres),
            _ => Err(Error::validation(
                "DATABASE_URL: unsupported scheme (expected sqlite://, postgres:// or postgresql://)",
            )),
        }
    }

    /// Human-readable engine name for logs.
    pub fn name(self) -> &'static str {
        match self {
            DbBackend::Sqlite => "sqlite",
            DbBackend::Postgres => "postgres",
        }
    }
}

/// A connected pool for one of the supported backends.
///
/// [`crate::build_state`] takes anything convertible into it, so callers that
/// hold a concrete `SqlitePool` / `PgPool` (tests, tools) can pass it directly.
#[derive(Clone)]
pub enum DbPool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl DbPool {
    pub fn backend(&self) -> DbBackend {
        match self {
            DbPool::Sqlite(_) => DbBackend::Sqlite,
            DbPool::Postgres(_) => DbBackend::Postgres,
        }
    }

    /// Apply the backend's embedded migrations ([`MIGRATOR`] / [`PG_MIGRATOR`]).
    pub async fn migrate(&self) -> Result<()> {
        let result = match self {
            DbPool::Sqlite(pool) => MIGRATOR.run(pool).await,
            DbPool::Postgres(pool) => PG_MIGRATOR.run(pool).await,
        };
        result.map_err(|e| Error::internal(format!("running database migrations: {e}")))
    }
}

impl From<SqlitePool> for DbPool {
    fn from(pool: SqlitePool) -> Self {
        DbPool::Sqlite(pool)
    }
}

impl From<PgPool> for DbPool {
    fn from(pool: PgPool) -> Self {
        DbPool::Postgres(pool)
    }
}

/// Connect to whichever backend `database_url` names (see
/// [`DbBackend::from_url`]). Migrations are run separately via
/// [`DbPool::migrate`].
pub async fn connect_database(database_url: &str, db: &config::DbConfig) -> Result<DbPool> {
    match DbBackend::from_url(database_url)? {
        DbBackend::Sqlite => Ok(DbPool::Sqlite(connect_pool(database_url, db).await?)),
        DbBackend::Postgres => Ok(DbPool::Postgres(connect_pg_pool(database_url, db).await?)),
    }
}

/// Connect to SQLite and create the pool — the single place SQLite connection
/// and locking behaviour is configured.
///
/// Everything here is driven by [`config::DbConfig`] (`DB_*` env vars) so an
/// operator can widen the timeouts on a slow disk without a rebuild:
///
///   * `create_if_missing` — a fresh deployment has no database file yet.
///   * `foreign_keys` — enabled per connection so `ON DELETE CASCADE` fires for
///     sessions / team_members / invites (SQLite leaves FKs off by default).
///   * `journal_mode = WAL` — readers don't block the writer, which is what
///     makes a single-binary tracker serve the UI while ingesting.
///   * `busy_timeout` — how long SQLite waits for the one write lock before
///     returning `SQLITE_BUSY`. It only helps transactions that declare their
///     write intent up front, which is why every write transaction goes through
///     `BEGIN IMMEDIATE` (see `adapters::sqlite::write`).
///   * `synchronous` — `NORMAL` under WAL by default: fewer fsyncs, so the write
///     lock is held for shorter stretches.
pub async fn connect_pool(database_url: &str, db: &config::DbConfig) -> Result<SqlitePool> {
    use sqlx::ConnectOptions;
    use sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};

    let synchronous = match db.synchronous {
        config::Synchronous::Normal => SqliteSynchronous::Normal,
        config::Synchronous::Full => SqliteSynchronous::Full,
    };

    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(Error::Db)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(synchronous)
        .busy_timeout(db.busy_timeout)
        .disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(db.max_connections)
        .acquire_timeout(db.acquire_timeout)
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// Connect to PostgreSQL and create the pool.
///
/// Only the engine-neutral `DB_*` knobs apply: `DB_MAX_CONNECTIONS` and
/// `DB_ACQUIRE_TIMEOUT_MS`. The SQLite locking knobs (`DB_BUSY_TIMEOUT_MS`,
/// `DB_SYNCHRONOUS`) have no Postgres meaning and are ignored; TLS and other
/// server settings come from the URL itself (e.g. `?sslmode=require`).
pub async fn connect_pg_pool(database_url: &str, db: &config::DbConfig) -> Result<PgPool> {
    use sqlx::ConnectOptions;

    let options = PgConnectOptions::from_str(database_url)
        .map_err(Error::Db)?
        .application_name("soika")
        .disable_statement_logging();

    let pool = PgPoolOptions::new()
        .max_connections(db.max_connections)
        .acquire_timeout(db.acquire_timeout)
        .connect_with(options)
        .await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_is_detected_from_the_url_scheme() {
        let cases = [
            ("sqlite://soika.db", DbBackend::Sqlite),
            ("sqlite::memory:", DbBackend::Sqlite),
            ("postgres://u:p@localhost/soika", DbBackend::Postgres),
            ("postgresql://localhost/soika", DbBackend::Postgres),
            ("POSTGRES://localhost/soika", DbBackend::Postgres),
        ];
        for (url, expected) in cases {
            assert_eq!(DbBackend::from_url(url).unwrap(), expected, "{url}");
        }
    }

    #[test]
    fn unknown_scheme_is_rejected() {
        for url in ["mysql://localhost/soika", "soika.db", ""] {
            let err = DbBackend::from_url(url).unwrap_err();
            assert!(matches!(err, Error::Validation(_)), "{url}: got {err:?}");
        }
    }

    #[test]
    fn migrators_are_independent_histories() {
        // Each backend keeps its own numbered history; Postgres starts from a
        // consolidated schema, so it is never longer than SQLite's.
        assert!(MIGRATOR.iter().count() >= PG_MIGRATOR.iter().count());
        assert!(PG_MIGRATOR.iter().any(|m| m.version == 1));
    }
}
