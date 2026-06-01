//! Application configuration (MVP §3) — loaded from environment variables.

use crate::error::{Error, Result};

/// Optional SMTP configuration. When `None`, email features degrade gracefully
/// to UI-only (MVP §3 / §12).
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: Option<String>,
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Display name of the single organization (`ORGANIZATION_NAME`).
    pub organization_name: String,
    /// sqlx connection string (`DATABASE_URL`).
    pub database_url: String,
    /// HTTP listen address (`BIND_ADDR`).
    pub bind_addr: String,
    /// Public base URL used in emails / invite links / DSNs (`BASE_URL`).
    pub base_url: String,
    /// Secret used to sign session cookies / invite tokens (`SECRET_KEY`).
    pub secret_key: String,
    /// Whether public self-registration is enabled (`ALLOW_SIGNUP`).
    pub allow_signup: bool,
    /// Built-in admin email (`ADMIN_EMAIL`).
    pub admin_email: Option<String>,
    /// Built-in admin initial password (`ADMIN_PASSWORD`).
    pub admin_password: Option<String>,
    /// Default max stored events per project (`DEFAULT_EVENTS_RETENTION`).
    pub default_events_retention: i64,
    /// Default age-based retention in days when a project's own value is 0
    /// (`DEFAULT_RETENTION_DAYS`); 0 disables age-based pruning (Story 7.1).
    pub default_retention_days: i64,
    /// Cron schedule for the retention cleanup job (`RETENTION_CRON`).
    pub retention_cron: String,
    /// Optional SMTP settings (`SMTP_*`).
    pub smtp: Option<SmtpConfig>,
}

impl Config {
    /// Load configuration from environment variables, applying MVP §3 defaults.
    ///
    /// Returns `Error::Validation` if a required variable (`SECRET_KEY`) is
    /// missing or a typed value fails to parse.
    pub fn from_env() -> Result<Self> {
        let organization_name = env_or("ORGANIZATION_NAME", "soika");
        let database_url = env_or("DATABASE_URL", "sqlite://soika.db");
        let bind_addr = env_or("BIND_ADDR", "0.0.0.0:8080");
        let base_url = env_or("BASE_URL", "http://localhost:8080");

        let secret_key =
            std::env::var("SECRET_KEY").map_err(|_| Error::validation("SECRET_KEY is required"))?;
        if secret_key.trim().is_empty() {
            return Err(Error::validation("SECRET_KEY must not be empty"));
        }

        let allow_signup = parse_bool(&env_or("ALLOW_SIGNUP", "false"));
        let admin_email = env_opt("ADMIN_EMAIL");
        let admin_password = env_opt("ADMIN_PASSWORD");

        let default_events_retention = env_or("DEFAULT_EVENTS_RETENTION", "1000")
            .parse::<i64>()
            .map_err(|e| Error::validation(format!("DEFAULT_EVENTS_RETENTION: {e}")))?;

        let default_retention_days = env_or("DEFAULT_RETENTION_DAYS", "0")
            .parse::<i64>()
            .map_err(|e| Error::validation(format!("DEFAULT_RETENTION_DAYS: {e}")))?;

        let retention_cron = env_or("RETENTION_CRON", "0 */15 * * * *");

        let smtp = Self::smtp_from_env()?;

        Ok(Config {
            organization_name,
            database_url,
            bind_addr,
            base_url,
            secret_key,
            allow_signup,
            admin_email,
            admin_password,
            default_events_retention,
            default_retention_days,
            retention_cron,
            smtp,
        })
    }

    /// Build the optional SMTP config; returns `Ok(None)` when `SMTP_HOST` is
    /// unset so email features degrade gracefully (MVP §3).
    fn smtp_from_env() -> Result<Option<SmtpConfig>> {
        let Some(host) = env_opt("SMTP_HOST") else {
            return Ok(None);
        };
        let port = env_or("SMTP_PORT", "587")
            .parse::<u16>()
            .map_err(|e| Error::validation(format!("SMTP_PORT: {e}")))?;
        Ok(Some(SmtpConfig {
            host,
            port,
            username: env_opt("SMTP_USERNAME"),
            password: env_opt("SMTP_PASSWORD"),
            from: env_opt("SMTP_FROM"),
        }))
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_opt(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}
