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

/// Connect to the database and create the pool. The bin crate owns running
/// migrations via [`MIGRATOR`].
///
/// `foreign_keys` is enabled per connection so `ON DELETE CASCADE` fires for
/// sessions / team_members / invites (SQLite leaves FKs off by default).
/// SQLite-specific connection setup is isolated here / in `main`.
pub async fn connect_pool(database_url: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(crate::error::Error::Db)?
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// Build the [`AppState`] from a connected pool and resolved config, wiring the
/// SQLite adapters and the SMTP-or-noop mailer.
pub fn build_state(pool: SqlitePool, config: Config) -> AppState {
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

    let login_guard = Arc::new(LoginGuard::new(config.lockout.clone()));

    AppState {
        config: Arc::new(config),
        users: Arc::new(SqliteUserRepository::new(pool.clone())),
        sessions: Arc::new(SqliteSessionRepository::new(pool.clone())),
        teams: Arc::new(SqliteTeamRepository::new(pool.clone())),
        projects: Arc::new(SqliteProjectRepository::new(pool.clone())),
        favorites: Arc::new(SqliteFavoriteRepository::new(pool.clone())),
        issues: Arc::new(SqliteIssueRepository::new(pool.clone())),
        events: Arc::new(SqliteEventRepository::new(pool.clone())),
        mute_rules: Arc::new(SqliteTagMuteRuleRepository::new(pool.clone())),
        invites: Arc::new(SqliteInviteRepository::new(pool.clone())),
        settings: Arc::new(SqliteSettingsRepository::new(pool)),
        mailer,
        clock: Arc::new(SystemClock),
        rate_limiter: Arc::new(RateLimiter::new(DEFAULT_LIMIT, DEFAULT_WINDOW)),
        login_guard,
        // The OIDC provider (if any) is wired separately after startup discovery
        // (fail-fast) via [`AppState::with_oidc`].
        oidc: None,
    }
}
