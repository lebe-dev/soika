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
///
/// `Debug` is hand-written to redact [`password`](Self::password) so the secret
/// never reaches a log line or panic message.
#[derive(Clone)]
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

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("from", &self.from)
            .field("tls", &self.tls)
            .finish()
    }
}

/// Optional Sentry telemetry configuration. When `None`, error reporting is
/// disabled and the SDK is never initialized (backend) nor exposed to the SPA
/// (frontend). A single DSN is shared by backend and frontend; the frontend DSN
/// is served only from the authenticated `/api/client-config` endpoint so it
/// never appears on an unauthenticated route.
///
/// `Debug` is hand-written to redact the [`dsn`](Self::dsn) (it embeds a secret
/// project key) so it never reaches a log line or panic message.
#[derive(Clone)]
pub struct SentryConfig {
    /// Sentry DSN both halves report to (`SENTRY_DSN`). Setting this enables
    /// error reporting.
    pub dsn: String,
    /// Optional environment tag attached to events (`SENTRY_ENVIRONMENT`), e.g.
    /// `production` / `staging`. `None` leaves it unset (Sentry's default).
    pub environment: Option<String>,
}

impl std::fmt::Debug for SentryConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SentryConfig")
            .field("dsn", &"***")
            .field("environment", &self.environment)
            .finish()
    }
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
///
/// `Debug` is hand-written to redact [`client_secret`](Self::client_secret) so
/// the secret never reaches a log line or panic message.
#[derive(Clone)]
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

impl std::fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcConfig")
            .field("kind", &self.kind)
            .field("issuer_url", &self.issuer_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"***")
            .field("redirect_url", &self.redirect_url)
            .field("scopes", &self.scopes)
            .field("provider_name", &self.provider_name)
            .field("allowed_email_domains", &self.allowed_email_domains)
            .field("require_approval", &self.require_approval)
            .finish()
    }
}

/// Optional passkey (WebAuthn) configuration. When `None`, passkey sign-in and
/// registration are disabled and every `/auth/passkey/*` route answers `404`.
///
/// WebAuthn binds credentials to a *relying party id* (`rp_id`) — an effective
/// domain — and validates the browser-reported `origin` against a fixed list.
/// Both are derived from `BASE_URL` by default, so a standard deployment only
/// has to set `PASSKEY_ENABLED=true`.
#[derive(Debug, Clone)]
pub struct PasskeyConfig {
    /// Relying party id (`PASSKEY_RP_ID`): the effective domain credentials are
    /// bound to, e.g. `errors.example.com`. Defaults to the host of `BASE_URL`.
    /// Changing it invalidates every registered credential.
    pub rp_id: String,
    /// Human-readable relying party name shown by the authenticator
    /// (`PASSKEY_RP_NAME`); defaults to `ORGANIZATION_NAME`.
    pub rp_name: String,
    /// Origin the SPA is served from (`PASSKEY_RP_ORIGIN`); defaults to `BASE_URL`.
    pub rp_origin: String,
    /// Additional accepted origins (`PASSKEY_EXTRA_ORIGINS`, comma-separated),
    /// e.g. the vite dev server during local development.
    pub extra_origins: Vec<String>,
    /// Accept origins on subdomains of `rp_id` (`PASSKEY_ALLOW_SUBDOMAINS`).
    pub allow_subdomains: bool,
    /// How long the browser is given to complete a ceremony
    /// (`PASSKEY_TIMEOUT_SECONDS`), passed to the authenticator as a hint.
    pub timeout: Duration,
    /// Lifetime of the signed challenge cookie holding the ceremony state
    /// (`PASSKEY_CHALLENGE_TTL_SECONDS`). Bounds how long an unfinished
    /// registration/authentication remains completable.
    pub challenge_ttl: Duration,
    /// Maximum credentials one user may register (`PASSKEY_MAX_PER_USER`).
    pub max_per_user: i64,
}

/// How durably SQLite flushes each commit (`PRAGMA synchronous`).
///
/// Under WAL, `Normal` is the documented safe choice: a commit is not fsynced
/// individually (only at checkpoints), which shortens how long a writer holds
/// the write lock — the difference between the two is that a power loss / OS
/// crash may lose the most recent transactions, never a corrupt database.
/// `Full` fsyncs every commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Synchronous {
    #[default]
    Normal,
    Full,
}

/// Bounded retry + batching policy for database writes.
///
/// SQLite allows exactly one writer at a time. Under concurrent ingestion a
/// write can be refused with `SQLITE_BUSY` ("database is locked"); the write is
/// retried with exponential backoff + jitter instead of failing the request, and
/// only a fully spent budget surfaces as [`crate::Error::DbBusy`].
#[derive(Debug, Clone)]
pub struct WritePolicy {
    /// Retries *after* the first attempt (`DB_WRITE_MAX_RETRIES`). 0 disables
    /// retrying.
    pub max_retries: u32,
    /// Delay before the first retry (`DB_WRITE_RETRY_BASE_MS`); doubles each
    /// attempt.
    pub retry_base: Duration,
    /// Upper bound on the backoff delay (`DB_WRITE_RETRY_MAX_MS`).
    pub retry_max: Duration,
    /// Rows deleted per statement by the retention sweep (`DB_DELETE_BATCH`).
    /// Bulk deletes are chunked so a sweep cannot hold the single write lock for
    /// seconds while events are being ingested.
    pub delete_batch: i64,
}

impl Default for WritePolicy {
    /// Matches the env-var defaults: 5 retries, 20 ms → 500 ms backoff, 500-row
    /// delete batches.
    fn default() -> Self {
        WritePolicy {
            max_retries: 5,
            retry_base: Duration::from_millis(20),
            retry_max: Duration::from_millis(500),
            delete_batch: 500,
        }
    }
}

impl WritePolicy {
    /// Backoff delay before retry `attempt` (0-based): `retry_base * 2^attempt`,
    /// capped at [`retry_max`](Self::retry_max) and then jittered by ±25% so
    /// concurrent writers don't retry in lockstep.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        let delay = self.retry_base.saturating_mul(factor).min(self.retry_max);
        let millis = delay.as_millis().max(1) as u64;
        let spread = (millis / 2).max(1);
        // millis - spread/… stays >= 1: `spread <= millis/2`, so the low end is
        // at least half the delay.
        let jittered = millis - spread / 2 + rand::random_range(0..=spread);
        Duration::from_millis(jittered)
    }
}

/// Database connection + concurrency settings (`DB_*`).
///
/// soika ships as a single binary on SQLite, where the whole file is guarded by
/// one write lock. These knobs are the levers that keep concurrent ingestion off
/// that lock's critical path; every one of them is settable from the environment
/// so an operator can widen the timeouts on a slow disk without a rebuild.
#[derive(Debug, Clone)]
pub struct DbConfig {
    /// Pool size (`DB_MAX_CONNECTIONS`). Readers scale with it; writers are
    /// serialized by SQLite regardless.
    pub max_connections: u32,
    /// How long SQLite itself waits for the write lock before returning
    /// `SQLITE_BUSY` (`DB_BUSY_TIMEOUT_MS`). Effective only for transactions that
    /// declare their intent to write up front (soika uses `BEGIN IMMEDIATE`).
    pub busy_timeout: Duration,
    /// How long a caller waits for a free pooled connection
    /// (`DB_ACQUIRE_TIMEOUT_MS`) before failing with a pool timeout.
    pub acquire_timeout: Duration,
    /// Commit durability (`DB_SYNCHRONOUS`: `normal` | `full`).
    pub synchronous: Synchronous,
    /// `Retry-After` (`DB_BUSY_RETRY_AFTER_SECS`) advertised to Sentry SDKs when
    /// ingestion answers `429` because the write budget was spent.
    pub busy_retry_after: Duration,
    /// Retry/batching policy for writes (`DB_WRITE_*`, `DB_DELETE_BATCH`).
    pub write: WritePolicy,
}

impl Default for DbConfig {
    /// Matches the env-var defaults: 8 connections, 5 s busy timeout, 10 s
    /// acquire timeout, `synchronous=NORMAL`, `Retry-After: 2`.
    fn default() -> Self {
        DbConfig {
            max_connections: 8,
            busy_timeout: Duration::from_secs(5),
            acquire_timeout: Duration::from_secs(10),
            synchronous: Synchronous::Normal,
            busy_retry_after: Duration::from_secs(2),
            write: WritePolicy::default(),
        }
    }
}

/// Fully resolved runtime configuration.
///
/// `Debug` is hand-written to redact [`secret_key`](Self::secret_key) so a stray
/// `tracing::debug!(?config)` or panic message can never dump it. The nested
/// `smtp` / `oidc` / `sentry` configs redact their own secrets via their own
/// `Debug` impls.
#[derive(Clone)]
pub struct Config {
    /// Display name of the single organization (`ORGANIZATION_NAME`).
    pub organization_name: String,
    /// sqlx connection string (`DATABASE_URL`).
    pub database_url: String,
    /// Database pool / locking / write-retry settings (`DB_*`).
    pub db: DbConfig,
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
    /// Optional passkey / WebAuthn settings (`PASSKEY_*`); `None` when disabled.
    pub passkey: Option<PasskeyConfig>,
    /// Optional Sentry telemetry settings (`SENTRY_*`); `None` when disabled.
    pub sentry: Option<SentryConfig>,
    /// Brute-force protection for password logins (`LOGIN_LOCKOUT_*`).
    pub lockout: LockoutConfig,
    /// IANA timezone name used by the frontend to display timestamps
    /// (`TIMEZONE`). Must be a valid `Intl.DateTimeFormat` zone; defaults to
    /// `UTC`. The backend itself always works in UTC; this value is only passed
    /// to the SPA via `ClientConfigView`.
    pub timezone: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("organization_name", &self.organization_name)
            .field("database_url", &self.database_url)
            .field("db", &self.db)
            .field("bind_addr", &self.bind_addr)
            .field("base_url", &self.base_url)
            .field("secret_key", &"***")
            .field("allow_signup", &self.allow_signup)
            .field("default_events_retention", &self.default_events_retention)
            .field("default_retention_days", &self.default_retention_days)
            .field("retention_cron", &self.retention_cron)
            .field("smtp", &self.smtp)
            .field("oidc", &self.oidc)
            .field("passkey", &self.passkey)
            .field("sentry", &self.sentry)
            .field("lockout", &self.lockout)
            .field("timezone", &self.timezone)
            .finish()
    }
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

        let db = Self::db_from_env()?;

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
        let passkey = Self::passkey_from_env(&base_url, &organization_name)?;
        let sentry = Self::sentry_from_env();
        let lockout = Self::lockout_from_env()?;
        let timezone = env_or("TIMEZONE", "UTC");

        Ok(Config {
            organization_name,
            database_url,
            db,
            bind_addr,
            base_url,
            secret_key,
            allow_signup,
            default_events_retention,
            default_retention_days,
            retention_cron,
            smtp,
            oidc,
            passkey,
            sentry,
            lockout,
            timezone,
        })
    }

    /// Build the database pool / locking / write-retry config, applying
    /// defaults.
    ///
    /// Every value is validated so a misconfiguration fails fast at boot rather
    /// than turning into a stream of failed writes under load. A zero
    /// `DB_BUSY_TIMEOUT_MS` is accepted (it means "don't wait in SQLite, let the
    /// retry loop handle it"); a zero pool size or delete batch is not.
    fn db_from_env() -> Result<DbConfig> {
        let defaults = DbConfig::default();

        let max_connections = env_or("DB_MAX_CONNECTIONS", &defaults.max_connections.to_string())
            .parse::<u32>()
            .map_err(|e| Error::validation(format!("DB_MAX_CONNECTIONS: {e}")))?;
        if max_connections == 0 {
            return Err(Error::validation("DB_MAX_CONNECTIONS must be at least 1"));
        }

        let busy_timeout = env_millis("DB_BUSY_TIMEOUT_MS", defaults.busy_timeout)?;

        let acquire_timeout = env_millis("DB_ACQUIRE_TIMEOUT_MS", defaults.acquire_timeout)?;
        if acquire_timeout.is_zero() {
            return Err(Error::validation(
                "DB_ACQUIRE_TIMEOUT_MS must be greater than 0",
            ));
        }

        let synchronous = match env_or("DB_SYNCHRONOUS", "normal")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "normal" => Synchronous::Normal,
            "full" => Synchronous::Full,
            other => {
                return Err(Error::validation(format!(
                    "DB_SYNCHRONOUS: unknown value '{other}' (expected 'normal' or 'full')"
                )));
            }
        };

        let busy_retry_after = env_secs(
            "DB_BUSY_RETRY_AFTER_SECS",
            defaults.busy_retry_after.as_secs(),
        )?;

        let write = Self::write_policy_from_env(defaults.write)?;

        Ok(DbConfig {
            max_connections,
            busy_timeout,
            acquire_timeout,
            synchronous,
            busy_retry_after,
            write,
        })
    }

    /// Build the write retry/batching policy from `DB_WRITE_*` / `DB_DELETE_BATCH`.
    fn write_policy_from_env(defaults: WritePolicy) -> Result<WritePolicy> {
        let max_retries = env_or("DB_WRITE_MAX_RETRIES", &defaults.max_retries.to_string())
            .parse::<u32>()
            .map_err(|e| Error::validation(format!("DB_WRITE_MAX_RETRIES: {e}")))?;

        let retry_base = env_millis("DB_WRITE_RETRY_BASE_MS", defaults.retry_base)?;
        let retry_max = env_millis("DB_WRITE_RETRY_MAX_MS", defaults.retry_max)?;
        if retry_max < retry_base {
            return Err(Error::validation(
                "DB_WRITE_RETRY_MAX_MS must be >= DB_WRITE_RETRY_BASE_MS",
            ));
        }

        let delete_batch = env_or("DB_DELETE_BATCH", &defaults.delete_batch.to_string())
            .parse::<i64>()
            .map_err(|e| Error::validation(format!("DB_DELETE_BATCH: {e}")))?;
        if delete_batch < 1 {
            return Err(Error::validation("DB_DELETE_BATCH must be at least 1"));
        }

        Ok(WritePolicy {
            max_retries,
            retry_base,
            retry_max,
            delete_batch,
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

    /// Build the optional passkey config; returns `Ok(None)` when
    /// `PASSKEY_ENABLED` is not truthy.
    ///
    /// Everything defaults off `BASE_URL`: the relying party id is its host and
    /// the accepted origin is the base URL itself, so enabling passkeys on a
    /// correctly-configured deployment needs no further variables. A blank
    /// host (a malformed `BASE_URL`) fails fast rather than producing an
    /// unusable relying party.
    fn passkey_from_env(base_url: &str, organization_name: &str) -> Result<Option<PasskeyConfig>> {
        if !parse_bool(&env_or("PASSKEY_ENABLED", "false")) {
            return Ok(None);
        }

        let rp_id = match env_opt("PASSKEY_RP_ID") {
            Some(value) => value.trim().to_string(),
            None => host_from_url(base_url).ok_or_else(|| {
                Error::validation(
                    "PASSKEY_RP_ID could not be derived from BASE_URL; set it explicitly",
                )
            })?,
        };
        if rp_id.is_empty() {
            return Err(Error::validation("PASSKEY_RP_ID must not be empty"));
        }

        let rp_name = env_or("PASSKEY_RP_NAME", organization_name);
        let rp_origin = env_opt("PASSKEY_RP_ORIGIN")
            .unwrap_or_else(|| base_url.trim_end_matches('/').to_string());
        let extra_origins = split_list(&env_opt("PASSKEY_EXTRA_ORIGINS").unwrap_or_default(), ',');
        let allow_subdomains = parse_bool(&env_or("PASSKEY_ALLOW_SUBDOMAINS", "false"));

        let timeout = env_secs("PASSKEY_TIMEOUT_SECONDS", 60)?;
        if timeout.is_zero() {
            return Err(Error::validation(
                "PASSKEY_TIMEOUT_SECONDS must be greater than 0",
            ));
        }

        let challenge_ttl = env_secs("PASSKEY_CHALLENGE_TTL_SECONDS", 300)?;
        if challenge_ttl.is_zero() {
            return Err(Error::validation(
                "PASSKEY_CHALLENGE_TTL_SECONDS must be greater than 0",
            ));
        }

        let max_per_user = env_or("PASSKEY_MAX_PER_USER", "10")
            .parse::<i64>()
            .map_err(|e| Error::validation(format!("PASSKEY_MAX_PER_USER: {e}")))?;
        if max_per_user < 1 {
            return Err(Error::validation("PASSKEY_MAX_PER_USER must be at least 1"));
        }

        Ok(Some(PasskeyConfig {
            rp_id,
            rp_name,
            rp_origin,
            extra_origins,
            allow_subdomains,
            timeout,
            challenge_ttl,
            max_per_user,
        }))
    }
}

/// Extract the host of an absolute URL (no scheme, userinfo or port).
///
/// Deliberately hand-rolled: `config` stays free of URL/WebAuthn dependencies,
/// and the only shapes it must handle are the `BASE_URL` values an operator
/// writes. Returns `None` when no host can be found.
fn host_from_url(url: &str) -> Option<String> {
    let without_scheme = url.trim().split_once("://").map(|(_, rest)| rest)?;
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    // Drop userinfo (`user:pass@host`) and the port.
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host.strip_prefix('[') {
        // IPv6 literal: keep the brackets' contents, port follows the `]`.
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
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

/// Read a duration in whole milliseconds from `key`, falling back to `default`.
fn env_millis(key: &str, default: Duration) -> Result<Duration> {
    let millis = env_or(key, &default.as_millis().to_string())
        .parse::<u64>()
        .map_err(|e| Error::validation(format!("{key}: {e}")))?;
    Ok(Duration::from_millis(millis))
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

    const DB_KEYS: &[&str] = &[
        "DB_MAX_CONNECTIONS",
        "DB_BUSY_TIMEOUT_MS",
        "DB_ACQUIRE_TIMEOUT_MS",
        "DB_SYNCHRONOUS",
        "DB_BUSY_RETRY_AFTER_SECS",
        "DB_WRITE_MAX_RETRIES",
        "DB_WRITE_RETRY_BASE_MS",
        "DB_WRITE_RETRY_MAX_MS",
        "DB_DELETE_BATCH",
    ];

    fn clear_db_env() {
        for k in DB_KEYS {
            unsafe { std::env::remove_var(k) };
        }
    }

    #[test]
    fn db_config_defaults_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_db_env();

        let db = Config::db_from_env().expect("defaults are valid");

        assert_eq!(db.max_connections, 8);
        assert_eq!(db.busy_timeout, Duration::from_secs(5));
        assert_eq!(db.acquire_timeout, Duration::from_secs(10));
        assert_eq!(db.synchronous, Synchronous::Normal);
        assert_eq!(db.busy_retry_after, Duration::from_secs(2));
        assert_eq!(db.write.max_retries, 5);
        assert_eq!(db.write.retry_base, Duration::from_millis(20));
        assert_eq!(db.write.retry_max, Duration::from_millis(500));
        assert_eq!(db.write.delete_batch, 500);
    }

    #[test]
    fn db_config_reads_every_knob_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_db_env();
        unsafe {
            std::env::set_var("DB_MAX_CONNECTIONS", "4");
            std::env::set_var("DB_BUSY_TIMEOUT_MS", "12000");
            std::env::set_var("DB_ACQUIRE_TIMEOUT_MS", "7000");
            std::env::set_var("DB_SYNCHRONOUS", "FULL");
            std::env::set_var("DB_BUSY_RETRY_AFTER_SECS", "9");
            std::env::set_var("DB_WRITE_MAX_RETRIES", "10");
            std::env::set_var("DB_WRITE_RETRY_BASE_MS", "30");
            std::env::set_var("DB_WRITE_RETRY_MAX_MS", "900");
            std::env::set_var("DB_DELETE_BATCH", "250");
        }

        let db = Config::db_from_env().expect("valid config");

        assert_eq!(db.max_connections, 4);
        assert_eq!(db.busy_timeout, Duration::from_millis(12_000));
        assert_eq!(db.acquire_timeout, Duration::from_millis(7_000));
        assert_eq!(db.synchronous, Synchronous::Full);
        assert_eq!(db.busy_retry_after, Duration::from_secs(9));
        assert_eq!(db.write.max_retries, 10);
        assert_eq!(db.write.retry_base, Duration::from_millis(30));
        assert_eq!(db.write.retry_max, Duration::from_millis(900));
        assert_eq!(db.write.delete_batch, 250);

        clear_db_env();
    }

    #[test]
    fn db_config_rejects_invalid_values() {
        let _guard = ENV_LOCK.lock().unwrap();

        for (key, value) in [
            ("DB_MAX_CONNECTIONS", "0"),
            ("DB_MAX_CONNECTIONS", "many"),
            ("DB_ACQUIRE_TIMEOUT_MS", "0"),
            ("DB_SYNCHRONOUS", "off"),
            ("DB_DELETE_BATCH", "0"),
            ("DB_WRITE_MAX_RETRIES", "-1"),
        ] {
            clear_db_env();
            unsafe { std::env::set_var(key, value) };
            let err = Config::db_from_env().expect_err(&format!("{key}={value} must be rejected"));
            assert!(
                matches!(err, Error::Validation(_)),
                "{key}={value} gave {err}"
            );
        }

        // retry_max below retry_base is a cross-field violation.
        clear_db_env();
        unsafe {
            std::env::set_var("DB_WRITE_RETRY_BASE_MS", "500");
            std::env::set_var("DB_WRITE_RETRY_MAX_MS", "100");
        }
        let err = Config::db_from_env().expect_err("retry_max < retry_base must be rejected");
        assert!(matches!(err, Error::Validation(_)), "got {err}");

        clear_db_env();
    }

    #[test]
    fn write_policy_backoff_grows_and_is_capped() {
        let policy = WritePolicy {
            max_retries: 8,
            retry_base: Duration::from_millis(20),
            retry_max: Duration::from_millis(200),
            delete_batch: 500,
        };

        // Jitter is +/-25%, so assert the band rather than an exact value.
        for (attempt, expected) in [(0u32, 20u64), (1, 40), (2, 80), (3, 160)] {
            let delay = policy.backoff(attempt).as_millis() as u64;
            assert!(
                delay >= expected * 3 / 4 && delay <= expected * 5 / 4,
                "attempt {attempt}: {delay}ms outside +/-25% of {expected}ms"
            );
        }

        // Capped, jitter included.
        for attempt in [4u32, 5, 31, 40] {
            let delay = policy.backoff(attempt).as_millis() as u64;
            assert!(delay <= 250, "attempt {attempt}: {delay}ms exceeds the cap");
        }
    }

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

    const PASSKEY_KEYS: &[&str] = &[
        "PASSKEY_ENABLED",
        "PASSKEY_RP_ID",
        "PASSKEY_RP_NAME",
        "PASSKEY_RP_ORIGIN",
        "PASSKEY_EXTRA_ORIGINS",
        "PASSKEY_ALLOW_SUBDOMAINS",
        "PASSKEY_TIMEOUT_SECONDS",
        "PASSKEY_CHALLENGE_TTL_SECONDS",
        "PASSKEY_MAX_PER_USER",
    ];

    fn clear_passkey_env() {
        for k in PASSKEY_KEYS {
            unsafe { std::env::remove_var(k) };
        }
    }

    #[test]
    fn passkey_disabled_yields_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_passkey_env();

        let passkey = Config::passkey_from_env("https://errors.example.com", "soika").unwrap();
        assert!(passkey.is_none());

        unsafe { std::env::set_var("PASSKEY_ENABLED", "false") };
        let passkey = Config::passkey_from_env("https://errors.example.com", "soika").unwrap();
        assert!(passkey.is_none());

        clear_passkey_env();
    }

    #[test]
    fn passkey_enabled_defaults_derive_from_base_url() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_passkey_env();
        unsafe { std::env::set_var("PASSKEY_ENABLED", "true") };

        let passkey = Config::passkey_from_env("https://errors.example.com/", "Acme")
            .unwrap()
            .expect("passkey should be Some when enabled");

        assert_eq!(passkey.rp_id, "errors.example.com");
        assert_eq!(passkey.rp_origin, "https://errors.example.com");
        assert_eq!(passkey.rp_name, "Acme");
        assert!(passkey.extra_origins.is_empty());
        assert!(!passkey.allow_subdomains);
        assert_eq!(passkey.timeout, Duration::from_secs(60));
        assert_eq!(passkey.challenge_ttl, Duration::from_secs(300));
        assert_eq!(passkey.max_per_user, 10);

        clear_passkey_env();
    }

    #[test]
    fn passkey_enabled_honours_overrides() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_passkey_env();
        unsafe { std::env::set_var("PASSKEY_ENABLED", "true") };
        unsafe { std::env::set_var("PASSKEY_RP_ID", "example.com") };
        unsafe { std::env::set_var("PASSKEY_RP_NAME", "Soika SSO") };
        unsafe { std::env::set_var("PASSKEY_RP_ORIGIN", "https://app.example.com") };
        unsafe {
            std::env::set_var(
                "PASSKEY_EXTRA_ORIGINS",
                "http://localhost:4200, http://localhost:8080",
            )
        };
        unsafe { std::env::set_var("PASSKEY_ALLOW_SUBDOMAINS", "true") };
        unsafe { std::env::set_var("PASSKEY_MAX_PER_USER", "3") };

        let passkey = Config::passkey_from_env("https://errors.example.com", "soika")
            .unwrap()
            .expect("passkey should be Some when enabled");

        assert_eq!(passkey.rp_id, "example.com");
        assert_eq!(passkey.rp_name, "Soika SSO");
        assert_eq!(passkey.rp_origin, "https://app.example.com");
        assert_eq!(
            passkey.extra_origins,
            vec!["http://localhost:4200", "http://localhost:8080"]
        );
        assert!(passkey.allow_subdomains);
        assert_eq!(passkey.max_per_user, 3);

        clear_passkey_env();
    }

    #[test]
    fn passkey_rejects_invalid_quota() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_passkey_env();
        unsafe { std::env::set_var("PASSKEY_ENABLED", "true") };
        unsafe { std::env::set_var("PASSKEY_MAX_PER_USER", "0") };

        let err = Config::passkey_from_env("https://errors.example.com", "soika").unwrap_err();
        assert!(matches!(err, Error::Validation(_)), "got {err:?}");

        clear_passkey_env();
    }

    #[test]
    fn passkey_without_derivable_host_is_a_validation_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_passkey_env();
        unsafe { std::env::set_var("PASSKEY_ENABLED", "true") };

        let err = Config::passkey_from_env("not-a-url", "soika").unwrap_err();
        assert!(matches!(err, Error::Validation(_)), "got {err:?}");

        clear_passkey_env();
    }

    #[test]
    fn host_from_url_handles_ports_userinfo_and_ipv6() {
        assert_eq!(
            host_from_url("https://errors.example.com/path?x=1"),
            Some("errors.example.com".to_string())
        );
        assert_eq!(
            host_from_url("http://localhost:8080"),
            Some("localhost".to_string())
        );
        assert_eq!(
            host_from_url("https://user:pass@Errors.Example.COM:8443/"),
            Some("errors.example.com".to_string())
        );
        assert_eq!(host_from_url("http://[::1]:8080/"), Some("::1".to_string()));
        assert_eq!(host_from_url("localhost:8080"), None);
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
