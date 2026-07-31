//! soika — lightweight, Sentry-compatible error tracking (library crate).
//!
//! Hexagonal architecture: domain & service logic depend on the
//! repository TRAITS in [`ports`]; infrastructure in [`adapters`] implements
//! them. The lib crate uses `thiserror` (see [`error::Error`]); the bin crate
//! uses `anyhow`.

pub mod adapters;
pub mod api;
pub mod auth;
pub mod config;
pub mod domain;
pub mod error;
pub mod grouping;
pub mod ingest;
pub mod mail;
pub mod notify;
pub mod ports;
pub mod router;
pub mod scheduler;
pub mod state;
pub mod web;

pub use config::Config;
pub use error::{Error, Result};
pub use state::AppState;

use std::str::FromStr;
use std::sync::Arc;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// Embedded SQL migrations (schema), run at startup.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Connect to the database and create the pool — the single place SQLite
/// connection and locking behaviour is configured. The bin crate owns running
/// migrations via [`MIGRATOR`].
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
        .map_err(crate::error::Error::Db)?
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

/// Build the [`AppState`] from a connected pool and resolved config, wiring the
/// SQLite adapters and the SMTP-or-noop mailer.
///
/// Loads the email templates (from [`mail::templates_dir`]) up front so a
/// missing or broken `templates/` directory fails fast at startup rather than
/// at the first notification.
pub fn build_state(pool: SqlitePool, config: Config) -> Result<AppState> {
    use adapters::clock::SystemClock;
    use adapters::mailer::{NoopMailer, SmtpMailer};
    use adapters::sqlite::{
        SqliteEventRepository, SqliteFavoriteRepository, SqliteInviteRepository,
        SqliteIssueRepository, SqliteProjectRepository, SqliteSessionRepository,
        SqliteSettingsRepository, SqliteTagMuteRuleRepository, SqliteTeamRepository,
        SqliteUserRepository,
    };
    use auth::LoginGuard;
    use ingest::{DEFAULT_LIMIT, DEFAULT_WINDOW, RateLimiter};
    use ports::Mailer;

    let mailer: Arc<dyn Mailer> = match config.smtp.clone() {
        Some(smtp) => Arc::new(SmtpMailer::new(smtp)),
        None => Arc::new(NoopMailer),
    };

    let templates = Arc::new(mail::Templates::load(&mail::templates_dir())?);

    let login_guard = Arc::new(LoginGuard::new(config.lockout.clone()));

    // Read before `config` is moved into the state below. The write policy is
    // only wired into the repositories on the ingest / retention hot path —
    // everything else keeps the defaults.
    let write_policy = config.db.write.clone();

    Ok(AppState {
        config: Arc::new(config),
        users: Arc::new(SqliteUserRepository::new(pool.clone())),
        sessions: Arc::new(SqliteSessionRepository::new(pool.clone())),
        teams: Arc::new(SqliteTeamRepository::new(pool.clone())),
        projects: Arc::new(SqliteProjectRepository::new(pool.clone())),
        favorites: Arc::new(SqliteFavoriteRepository::new(pool.clone())),
        issues: Arc::new(
            SqliteIssueRepository::new(pool.clone()).with_write_policy(write_policy.clone()),
        ),
        events: Arc::new(SqliteEventRepository::new(pool.clone()).with_write_policy(write_policy)),
        mute_rules: Arc::new(SqliteTagMuteRuleRepository::new(pool.clone())),
        invites: Arc::new(SqliteInviteRepository::new(pool.clone())),
        settings: Arc::new(SqliteSettingsRepository::new(pool)),
        mailer,
        templates,
        clock: Arc::new(SystemClock),
        rate_limiter: Arc::new(RateLimiter::new(DEFAULT_LIMIT, DEFAULT_WINDOW)),
        login_guard,
        // The OIDC provider (if any) is wired separately after startup discovery
        // (fail-fast) via [`AppState::with_oidc`].
        oidc: None,
    })
}
