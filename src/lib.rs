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
pub mod db;
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

use std::sync::Arc;

pub use db::{
    DbBackend, DbPool, MIGRATOR, PG_MIGRATOR, connect_database, connect_pg_pool, connect_pool,
};

/// Build the [`AppState`] from a connected pool and resolved config, wiring the
/// repository adapters of the pool's backend (SQLite or PostgreSQL) and the
/// SMTP-or-noop mailer.
///
/// Loads the email templates (from [`mail::templates_dir`]) up front so a
/// missing or broken `templates/` directory fails fast at startup rather than
/// at the first notification.
pub fn build_state(pool: impl Into<DbPool>, config: Config) -> Result<AppState> {
    use adapters::clock::SystemClock;
    use adapters::mailer::{NoopMailer, SmtpMailer};
    use auth::LoginGuard;
    use ingest::{DEFAULT_LIMIT, DEFAULT_WINDOW, RateLimiter};
    use ports::Mailer;

    let mailer: Arc<dyn Mailer> = match config.smtp.clone() {
        Some(smtp) => Arc::new(SmtpMailer::new(smtp)),
        None => Arc::new(NoopMailer),
    };

    let templates = Arc::new(mail::Templates::load(&mail::templates_dir())?);

    let login_guard = Arc::new(LoginGuard::new(config.lockout.clone()));

    // The WebAuthn relying party is built here (not lazily per request) so a
    // misconfigured `PASSKEY_RP_ID` / origin fails at startup instead of on a
    // user's first sign-in attempt.
    let webauthn = config
        .passkey
        .as_ref()
        .map(auth::passkey::build_webauthn)
        .transpose()?
        .map(Arc::new);

    // Read before `config` is moved into the state below. The write policy is
    // only wired into the repositories on the ingest / retention hot path —
    // everything else keeps the defaults.
    let write_policy = config.db.write.clone();

    let repos = match pool.into() {
        DbPool::Sqlite(pool) => sqlite_repositories(pool, write_policy),
        DbPool::Postgres(pool) => postgres_repositories(pool, write_policy),
    };

    Ok(AppState {
        config: Arc::new(config),
        users: repos.users,
        sessions: repos.sessions,
        teams: repos.teams,
        projects: repos.projects,
        favorites: repos.favorites,
        issues: repos.issues,
        events: repos.events,
        mute_rules: repos.mute_rules,
        invites: repos.invites,
        passkeys: repos.passkeys,
        settings: repos.settings,
        mailer,
        templates,
        clock: Arc::new(SystemClock),
        rate_limiter: Arc::new(RateLimiter::new(DEFAULT_LIMIT, DEFAULT_WINDOW)),
        login_guard,
        // The OIDC provider (if any) is wired separately after startup discovery
        // (fail-fast) via [`AppState::with_oidc`].
        oidc: None,
        webauthn,
    })
}

/// Every repository port, as wired into [`AppState`].
struct Repositories {
    users: Arc<dyn ports::UserRepository>,
    sessions: Arc<dyn ports::SessionRepository>,
    teams: Arc<dyn ports::TeamRepository>,
    projects: Arc<dyn ports::ProjectRepository>,
    favorites: Arc<dyn ports::FavoriteRepository>,
    issues: Arc<dyn ports::IssueRepository>,
    events: Arc<dyn ports::EventRepository>,
    mute_rules: Arc<dyn ports::TagMuteRuleRepository>,
    invites: Arc<dyn ports::InviteRepository>,
    passkeys: Arc<dyn ports::PasskeyRepository>,
    settings: Arc<dyn ports::SettingsRepository>,
}

/// The SQLite adapters. The write policy is only wired into the repositories on
/// the ingest / retention hot path — everything else keeps the defaults.
fn sqlite_repositories(pool: sqlx::SqlitePool, write: config::WritePolicy) -> Repositories {
    use adapters::sqlite::{
        SqliteEventRepository, SqliteFavoriteRepository, SqliteInviteRepository,
        SqliteIssueRepository, SqlitePasskeyRepository, SqliteProjectRepository,
        SqliteSessionRepository, SqliteSettingsRepository, SqliteTagMuteRuleRepository,
        SqliteTeamRepository, SqliteUserRepository,
    };

    Repositories {
        users: Arc::new(SqliteUserRepository::new(pool.clone())),
        sessions: Arc::new(SqliteSessionRepository::new(pool.clone())),
        teams: Arc::new(SqliteTeamRepository::new(pool.clone())),
        projects: Arc::new(SqliteProjectRepository::new(pool.clone())),
        favorites: Arc::new(SqliteFavoriteRepository::new(pool.clone())),
        issues: Arc::new(SqliteIssueRepository::new(pool.clone()).with_write_policy(write.clone())),
        events: Arc::new(SqliteEventRepository::new(pool.clone()).with_write_policy(write)),
        mute_rules: Arc::new(SqliteTagMuteRuleRepository::new(pool.clone())),
        invites: Arc::new(SqliteInviteRepository::new(pool.clone())),
        passkeys: Arc::new(SqlitePasskeyRepository::new(pool.clone())),
        settings: Arc::new(SqliteSettingsRepository::new(pool)),
    }
}

/// The PostgreSQL adapters, wired like [`sqlite_repositories`].
fn postgres_repositories(pool: sqlx::PgPool, write: config::WritePolicy) -> Repositories {
    use adapters::postgres::{
        PgEventRepository, PgFavoriteRepository, PgInviteRepository, PgIssueRepository,
        PgPasskeyRepository, PgProjectRepository, PgSessionRepository, PgSettingsRepository,
        PgTagMuteRuleRepository, PgTeamRepository, PgUserRepository,
    };

    Repositories {
        users: Arc::new(PgUserRepository::new(pool.clone())),
        sessions: Arc::new(PgSessionRepository::new(pool.clone())),
        teams: Arc::new(PgTeamRepository::new(pool.clone())),
        projects: Arc::new(PgProjectRepository::new(pool.clone())),
        favorites: Arc::new(PgFavoriteRepository::new(pool.clone())),
        issues: Arc::new(PgIssueRepository::new(pool.clone()).with_write_policy(write.clone())),
        events: Arc::new(PgEventRepository::new(pool.clone()).with_write_policy(write)),
        mute_rules: Arc::new(PgTagMuteRuleRepository::new(pool.clone())),
        invites: Arc::new(PgInviteRepository::new(pool.clone())),
        passkeys: Arc::new(PgPasskeyRepository::new(pool.clone())),
        settings: Arc::new(PgSettingsRepository::new(pool)),
    }
}
