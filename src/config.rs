//! Application configuration — loaded from environment variables.

use std::time::Duration;

use crate::error::{Error, Result};

/// Brute-force protection for password logins.
///
/// Failed credential checks are counted per (client IP, email) key over a
/// rolling [`window`](Self::window). Once [`max_attempts`](Self::max_attempts)
/// failures accumulate the key is locked; each successive lockout in a sustained
/// attack grows exponentially from [`base_lockout`](Self::base_lockout) up to
/// [`max_lockout`](Self::max_lockout). A successful login clears the key.
#[derive(Debug, Clone)]
pub struct LockoutConfig {
    /// Master switch (`LOGIN_LOCKOUT_ENABLED`). When false the guard is a no-op.
    pub enabled: bool,
    /// Failures within `window` that trigger a lockout (`LOGIN_MAX_ATTEMPTS`).
    pub max_attempts: u32,
    /// Rolling window over which failures accumulate (`LOGIN_LOCKOUT_WINDOW_SECS`).
    pub window: Duration,
    /// First lockout duration; doubles on each repeat (`LOGIN_LOCKOUT_BASE_SECS`).
    pub base_lockout: Duration,
    /// Upper bound on the exponential lockout (`LOGIN_LOCKOUT_MAX_SECS`).
    pub max_lockout: Duration,
}

impl Default for LockoutConfig {
    /// Matches the env-var defaults: 5 attempts per 15 min, locking from 1 min
    /// up to 1 hour.
    fn default() -> Self {
        LockoutConfig {
            enabled: true,
            max_attempts: 5,
            window: Duration::from_secs(900),
            base_lockout: Duration::from_secs(60),
            max_lockout: Duration::from_secs(3600),
        }
    }
}

/// Optional SMTP configuration. When `None`, email features degrade gracefully
/// to UI-only.
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: Option<String>,
    /// Whether to encrypt the connection (`SMTP_TLS`, default `true`). When
    /// `false` the transport connects in plaintext with no TLS/STARTTLS — only
    /// for local relays such as mailcrab/MailHog that have no TLS support.
    pub tls: bool,
}

/// Optional Sentry telemetry configuration. When `None`, error reporting is
/// disabled and the SDK is never initialized (backend) nor exposed to the SPA
/// (frontend). A single DSN is shared by backend and frontend; the frontend DSN
/// is served only from the authenticated `/api/client-config` endpoint so it
/// never appears on an unauthenticated route.
#[derive(Debug, Clone)]
pub struct SentryConfig {
    /// Sentry DSN both halves report to (`SENTRY_DSN`). Setting this enables
    /// error reporting.
    pub dsn: String,
    /// Optional environment tag attached to events (`SENTRY_ENVIRONMENT`), e.g.
    /// `production` / `staging`. `None` leaves it unset (Sentry's default).
    pub environment: Option<String>,
}

/// Which OAuth provider flavour to speak.
///
/// `Oidc` is the generic OpenID Connect flow (discovery + ID-token), used by
/// GitLab, Google, Keycloak, Authentik, Okta, Entra, … `Github` is plain OAuth
/// 2.0 (no discovery, no ID-token): GitHub OAuth Apps are not OIDC providers, so
/// identity is read from the REST API instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProviderKind {
    Oidc,
    Github,
}

/// Optional OAuth 2.0 / OpenID Connect configuration. When `None`, SSO is
/// disabled and password login behaves exactly as before.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// Provider flavour (`OAUTH_PROVIDER`): generic `oidc` (default) or `github`.
    pub kind: OAuthProviderKind,
    /// Issuer base URL used for OIDC discovery (`OAUTH_ISSUER_URL`), e.g.
    /// `https://gitlab.com`. Discovery hits `{issuer}/.well-known/openid-configuration`.
    pub issuer_url: String,
    /// Client ID registered with the provider (`OAUTH_CLIENT_ID`).
    pub client_id: String,
    /// Client secret registered with the provider (`OAUTH_CLIENT_SECRET`).
    pub client_secret: String,
    /// Redirect URI; must match the provider config (`OAUTH_REDIRECT_URL`).
    /// Defaults to `{BASE_URL}/auth/oidc/callback`.
    pub redirect_url: String,
    /// Space-separated scopes (`OAUTH_SCOPES`); default `openid email profile`.
    pub scopes: Vec<String>,
    /// Human-readable provider name shown on the SSO button (`OAUTH_PROVIDER_NAME`).
    pub provider_name: String,
    /// Optional whitelist of allowed email domains for auto-provisioning
    /// (`OAUTH_ALLOWED_EMAIL_DOMAINS`, comma-separated). Empty = any domain.
    pub allowed_email_domains: Vec<String>,
    /// When true (`OAUTH_REQUIRE_APPROVAL`), a user provisioned on first SSO
    /// login is created in a `pending` status and receives no session until an
    /// instance admin approves the account. Default `false`.
    pub require_approval: bool,
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
    /// Default max stored events per project (`DEFAULT_EVENTS_RETENTION`).
    pub default_events_retention: i64,
    /// Default age-based retention in days when a project's own value is 0
    /// (`DEFAULT_RETENTION_DAYS`); 0 disables age-based pruning.
    pub default_retention_days: i64,
    /// Cron schedule for the retention cleanup job (`RETENTION_CRON`).
    pub retention_cron: String,
    /// Optional SMTP settings (`SMTP_*`).
    pub smtp: Option<SmtpConfig>,
    /// Optional OAuth / OIDC settings (`OAUTH_*`); `None` when disabled.
    pub oidc: Option<OidcConfig>,
    /// Optional Sentry telemetry settings (`SENTRY_*`); `None` when disabled.
    pub sentry: Option<SentryConfig>,
    /// Brute-force protection for password logins (`LOGIN_LOCKOUT_*`).
    pub lockout: LockoutConfig,
}

impl Config {
    /// Load configuration from environment variables, applying defaults.
    ///
    /// Returns `Error::Validation` if a required variable (`SECRET_KEY`) is
    /// missing or a typed value fails to parse.
    pub fn from_env() -> Result<Self> {
        let organization_name = env_or("ORGANIZATION_NAME", "soika");
        let database_url = env_or("DATABASE_URL", "sqlite://soika.db");
        let bind_addr = env_or("BIND_ADDR", "127.0.0.1:8080");
        let base_url = env_or("BASE_URL", "http://localhost:8080");

        let secret_key =
            std::env::var("SECRET_KEY").map_err(|_| Error::validation("SECRET_KEY is required"))?;
        if secret_key.trim().is_empty() {
            return Err(Error::validation("SECRET_KEY must not be empty"));
        }

        let allow_signup = parse_bool(&env_or("ALLOW_SIGNUP", "false"));

        let default_events_retention = env_or("DEFAULT_EVENTS_RETENTION", "1000")
            .parse::<i64>()
            .map_err(|e| Error::validation(format!("DEFAULT_EVENTS_RETENTION: {e}")))?;

        let default_retention_days = env_or("DEFAULT_RETENTION_DAYS", "0")
            .parse::<i64>()
            .map_err(|e| Error::validation(format!("DEFAULT_RETENTION_DAYS: {e}")))?;

        let retention_cron = env_or("RETENTION_CRON", "0 */15 * * * *");

        let smtp = Self::smtp_from_env()?;
        let oidc = Self::oidc_from_env(&base_url)?;
        let sentry = Self::sentry_from_env();
        let lockout = Self::lockout_from_env()?;

        Ok(Config {
            organization_name,
            database_url,
            bind_addr,
            base_url,
            secret_key,
            allow_signup,
            default_events_retention,
            default_retention_days,
            retention_cron,
            smtp,
            oidc,
            sentry,
            lockout,
        })
    }

    /// Build the optional Sentry config; returns `None` when `SENTRY_DSN` is
    /// unset or empty so error reporting degrades gracefully to off. The
    /// environment tag is optional.
    fn sentry_from_env() -> Option<SentryConfig> {
        let dsn = env_opt("SENTRY_DSN")?;
        Some(SentryConfig {
            dsn,
            environment: env_opt("SENTRY_ENVIRONMENT"),
        })
    }

    /// Build the login brute-force protection config, applying defaults. Enabled
    /// by default; values are validated so a misconfiguration fails fast at boot
    /// rather than silently disabling the guard.
    fn lockout_from_env() -> Result<LockoutConfig> {
        let enabled = parse_bool(&env_or("LOGIN_LOCKOUT_ENABLED", "true"));

        let max_attempts = env_or("LOGIN_MAX_ATTEMPTS", "5")
            .parse::<u32>()
            .map_err(|e| Error::validation(format!("LOGIN_MAX_ATTEMPTS: {e}")))?;
        if max_attempts == 0 {
            return Err(Error::validation("LOGIN_MAX_ATTEMPTS must be at least 1"));
        }

        let window = env_secs("LOGIN_LOCKOUT_WINDOW_SECS", 900)?;
        let base_lockout = env_secs("LOGIN_LOCKOUT_BASE_SECS", 60)?;
        let max_lockout = env_secs("LOGIN_LOCKOUT_MAX_SECS", 3600)?;
        if max_lockout < base_lockout {
            return Err(Error::validation(
                "LOGIN_LOCKOUT_MAX_SECS must be >= LOGIN_LOCKOUT_BASE_SECS",
            ));
        }

        Ok(LockoutConfig {
            enabled,
            max_attempts,
            window,
            base_lockout,
            max_lockout,
        })
    }

    /// Build the optional SMTP config; returns `Ok(None)` when `SMTP_HOST` is
    /// unset so email features degrade gracefully.
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
            tls: parse_bool(&env_or("SMTP_TLS", "true")),
        }))
    }

    /// Build the optional OIDC config; returns `Ok(None)` when `OAUTH_ENABLED`
    /// is not truthy. When enabled, the issuer URL, client id and client secret
    /// are required and missing any of them fails fast.
    fn oidc_from_env(base_url: &str) -> Result<Option<OidcConfig>> {
        if !parse_bool(&env_or("OAUTH_ENABLED", "false")) {
            return Ok(None);
        }

        let kind = match env_or("OAUTH_PROVIDER", "oidc")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "oidc" => OAuthProviderKind::Oidc,
            "github" => OAuthProviderKind::Github,
            other => {
                return Err(Error::validation(format!(
                    "OAUTH_PROVIDER: unknown value '{other}' (expected 'oidc' or 'github')"
                )));
            }
        };

        let client_id = required_env("OAUTH_CLIENT_ID")?;
        let client_secret = required_env("OAUTH_CLIENT_SECRET")?;

        // GitHub is not an OIDC provider: it has no discovery document, so the
        // issuer URL is informational and defaults to github.com. Generic OIDC
        // requires it (discovery hits `{issuer}/.well-known/openid-configuration`).
        let issuer_url = match kind {
            OAuthProviderKind::Github => env_or("OAUTH_ISSUER_URL", "https://github.com"),
            OAuthProviderKind::Oidc => required_env("OAUTH_ISSUER_URL")?,
        };

        let redirect_url = env_opt("OAUTH_REDIRECT_URL")
            .unwrap_or_else(|| format!("{}/auth/oidc/callback", base_url.trim_end_matches('/')));

        // GitHub uses its own scope names (`read:user user:email`) and has no
        // `openid` scope; generic OIDC uses the standard `openid email profile`.
        let default_scopes = match kind {
            OAuthProviderKind::Github => "read:user user:email",
            OAuthProviderKind::Oidc => "openid email profile",
        };
        let scopes = split_list(&env_or("OAUTH_SCOPES", default_scopes), ' ');
        let provider_name = env_or("OAUTH_PROVIDER_NAME", "SSO");
        let allowed_email_domains = split_list(
            &env_opt("OAUTH_ALLOWED_EMAIL_DOMAINS").unwrap_or_default(),
            ',',
        );
        let require_approval = parse_bool(&env_or("OAUTH_REQUIRE_APPROVAL", "false"));

        Ok(Some(OidcConfig {
            kind,
            issuer_url,
            client_id,
            client_secret,
            redirect_url,
            scopes,
            provider_name,
            allowed_email_domains,
            require_approval,
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

/// Read a duration in whole seconds from `key`, falling back to `default_secs`.
fn env_secs(key: &str, default_secs: u64) -> Result<Duration> {
    let secs = env_or(key, &default_secs.to_string())
        .parse::<u64>()
        .map_err(|e| Error::validation(format!("{key}: {e}")))?;
    Ok(Duration::from_secs(secs))
}

fn parse_bool(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Read a required (non-empty) env var, failing fast with a validation error.
fn required_env(key: &str) -> Result<String> {
    env_opt(key).ok_or_else(|| Error::validation(format!("{key} is required when OAUTH_ENABLED")))
}

/// Split a delimited list into trimmed, non-empty items.
fn split_list(s: &str, sep: char) -> Vec<String> {
    s.split(sep)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::sync::Mutex;

    // Tests mutate process-wide env vars, so serialize them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const OAUTH_KEYS: &[&str] = &[
        "OAUTH_ENABLED",
        "OAUTH_PROVIDER",
        "OAUTH_ISSUER_URL",
        "OAUTH_CLIENT_ID",
        "OAUTH_CLIENT_SECRET",
        "OAUTH_REDIRECT_URL",
        "OAUTH_SCOPES",
        "OAUTH_PROVIDER_NAME",
        "OAUTH_ALLOWED_EMAIL_DOMAINS",
    ];

    fn clear_oauth_env() {
        for k in OAUTH_KEYS {
            unsafe { std::env::remove_var(k) };
        }
    }

    #[test]
    fn oidc_disabled_yields_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();
        unsafe { std::env::set_var("OAUTH_ENABLED", "false") };

        let oidc = Config::oidc_from_env("http://localhost:8080").unwrap();
        assert!(oidc.is_none());

        clear_oauth_env();
    }

    #[test]
    fn oidc_unset_yields_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();

        let oidc = Config::oidc_from_env("http://localhost:8080").unwrap();
        assert!(oidc.is_none());
    }

    #[test]
    fn oidc_enabled_missing_required_is_validation_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();
        unsafe { std::env::set_var("OAUTH_ENABLED", "true") };
        // Provide issuer + client id but omit the secret.
        unsafe { std::env::set_var("OAUTH_ISSUER_URL", "https://gitlab.com") };
        unsafe { std::env::set_var("OAUTH_CLIENT_ID", "abc") };

        let err = Config::oidc_from_env("http://localhost:8080").unwrap_err();
        assert!(matches!(err, Error::Validation(_)), "got {err:?}");

        clear_oauth_env();
    }

    #[test]
    fn github_provider_skips_issuer_requirement_and_defaults_scopes() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();
        unsafe { std::env::set_var("OAUTH_ENABLED", "true") };
        unsafe { std::env::set_var("OAUTH_PROVIDER", "github") };
        // No OAUTH_ISSUER_URL set: GitHub does not need one (no discovery).
        unsafe { std::env::set_var("OAUTH_CLIENT_ID", "gh-client") };
        unsafe { std::env::set_var("OAUTH_CLIENT_SECRET", "gh-secret") };

        let oidc = Config::oidc_from_env("https://errors.example.com")
            .unwrap()
            .expect("oidc should be Some when enabled");

        assert_eq!(oidc.kind, OAuthProviderKind::Github);
        assert_eq!(oidc.issuer_url, "https://github.com");
        // GitHub-flavoured default scopes (no `openid`).
        assert_eq!(oidc.scopes, vec!["read:user", "user:email"]);
        assert_eq!(
            oidc.redirect_url,
            "https://errors.example.com/auth/oidc/callback"
        );

        clear_oauth_env();
    }

    #[test]
    fn unknown_provider_is_validation_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();
        unsafe { std::env::set_var("OAUTH_ENABLED", "true") };
        unsafe { std::env::set_var("OAUTH_PROVIDER", "facebook") };
        unsafe { std::env::set_var("OAUTH_CLIENT_ID", "id") };
        unsafe { std::env::set_var("OAUTH_CLIENT_SECRET", "secret") };

        let err = Config::oidc_from_env("https://base").unwrap_err();
        assert!(matches!(err, Error::Validation(_)), "got {err:?}");

        clear_oauth_env();
    }

    #[test]
    fn oidc_provider_still_requires_issuer() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();
        unsafe { std::env::set_var("OAUTH_ENABLED", "true") };
        // Default provider is oidc; issuer is mandatory there.
        unsafe { std::env::set_var("OAUTH_CLIENT_ID", "id") };
        unsafe { std::env::set_var("OAUTH_CLIENT_SECRET", "secret") };

        let err = Config::oidc_from_env("https://base").unwrap_err();
        assert!(matches!(err, Error::Validation(_)), "got {err:?}");

        clear_oauth_env();
    }

    #[test]
    fn oidc_enabled_defaults_redirect_from_base_url() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();
        unsafe { std::env::set_var("OAUTH_ENABLED", "1") };
        unsafe { std::env::set_var("OAUTH_ISSUER_URL", "https://gitlab.com") };
        unsafe { std::env::set_var("OAUTH_CLIENT_ID", "client-id") };
        unsafe { std::env::set_var("OAUTH_CLIENT_SECRET", "client-secret") };

        let oidc = Config::oidc_from_env("https://errors.example.com")
            .unwrap()
            .expect("oidc should be Some when enabled");

        assert_eq!(
            oidc.redirect_url,
            "https://errors.example.com/auth/oidc/callback"
        );
        // Defaults.
        assert_eq!(oidc.scopes, vec!["openid", "email", "profile"]);
        assert_eq!(oidc.provider_name, "SSO");
        assert!(oidc.allowed_email_domains.is_empty());

        clear_oauth_env();
    }

    const SENTRY_KEYS: &[&str] = &["SENTRY_DSN", "SENTRY_ENVIRONMENT"];

    fn clear_sentry_env() {
        for k in SENTRY_KEYS {
            unsafe { std::env::remove_var(k) };
        }
    }

    #[test]
    fn sentry_unset_yields_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_sentry_env();

        assert!(Config::sentry_from_env().is_none());
    }

    #[test]
    fn sentry_blank_dsn_yields_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_sentry_env();
        // `env_opt` treats whitespace-only values as unset.
        unsafe { std::env::set_var("SENTRY_DSN", "   ") };

        assert!(Config::sentry_from_env().is_none());

        clear_sentry_env();
    }

    #[test]
    fn sentry_dsn_only_defaults_environment_to_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_sentry_env();
        unsafe { std::env::set_var("SENTRY_DSN", "https://pub@example.com/42") };

        let sentry = Config::sentry_from_env().expect("sentry should be Some when DSN set");
        assert_eq!(sentry.dsn, "https://pub@example.com/42");
        assert!(sentry.environment.is_none());

        clear_sentry_env();
    }

    #[test]
    fn sentry_honours_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_sentry_env();
        unsafe { std::env::set_var("SENTRY_DSN", "https://pub@example.com/42") };
        unsafe { std::env::set_var("SENTRY_ENVIRONMENT", "staging") };

        let sentry = Config::sentry_from_env().expect("sentry should be Some when DSN set");
        assert_eq!(sentry.environment.as_deref(), Some("staging"));

        clear_sentry_env();
    }

    const SMTP_KEYS: &[&str] = &[
        "SMTP_HOST",
        "SMTP_PORT",
        "SMTP_USERNAME",
        "SMTP_PASSWORD",
        "SMTP_FROM",
        "SMTP_TLS",
    ];

    fn clear_smtp_env() {
        for k in SMTP_KEYS {
            unsafe { std::env::remove_var(k) };
        }
    }

    #[test]
    fn smtp_unset_yields_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_smtp_env();

        assert!(Config::smtp_from_env().unwrap().is_none());
    }

    #[test]
    fn smtp_tls_defaults_to_true() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_smtp_env();
        unsafe { std::env::set_var("SMTP_HOST", "smtp.example.com") };

        let smtp = Config::smtp_from_env()
            .unwrap()
            .expect("smtp should be Some when host set");
        assert!(smtp.tls);

        clear_smtp_env();
    }

    #[test]
    fn smtp_tls_can_be_disabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_smtp_env();
        unsafe { std::env::set_var("SMTP_HOST", "localhost") };
        unsafe { std::env::set_var("SMTP_TLS", "false") };

        let smtp = Config::smtp_from_env()
            .unwrap()
            .expect("smtp should be Some when host set");
        assert!(!smtp.tls);

        clear_smtp_env();
    }

    #[test]
    fn oidc_enabled_honours_overrides() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_oauth_env();
        unsafe { std::env::set_var("OAUTH_ENABLED", "yes") };
        unsafe { std::env::set_var("OAUTH_ISSUER_URL", "https://gitlab.com") };
        unsafe { std::env::set_var("OAUTH_CLIENT_ID", "client-id") };
        unsafe { std::env::set_var("OAUTH_CLIENT_SECRET", "client-secret") };
        unsafe { std::env::set_var("OAUTH_REDIRECT_URL", "https://custom/cb") };
        unsafe { std::env::set_var("OAUTH_SCOPES", "openid email") };
        unsafe { std::env::set_var("OAUTH_PROVIDER_NAME", "GitLab") };
        unsafe { std::env::set_var("OAUTH_ALLOWED_EMAIL_DOMAINS", "example.com, itkey.com ,") };

        let oidc = Config::oidc_from_env("https://base")
            .unwrap()
            .expect("oidc should be Some when enabled");

        assert_eq!(oidc.redirect_url, "https://custom/cb");
        assert_eq!(oidc.scopes, vec!["openid", "email"]);
        assert_eq!(oidc.provider_name, "GitLab");
        assert_eq!(oidc.allowed_email_domains, vec!["example.com", "itkey.com"]);

        clear_oauth_env();
    }
}
