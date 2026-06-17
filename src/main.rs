//! soika binary — wiring, config, axum bootstrap, graceful shutdown.
//!
//! The bin crate uses `anyhow` for error handling.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result};
use sqlx::ConnectOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use tokio::net::TcpListener;
use tokio::signal;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use soika::auth::{GithubProvider, OidcClient, OidcProvider};
use soika::config::{OAuthProviderKind, SentryConfig};
use soika::{Config, MIGRATOR, build_state, scheduler};

#[tokio::main]
async fn main() -> Result<()> {
    // Load config before anything else: Sentry (and the tracing layer that feeds
    // it) must be initialized from it, and a bad config should fail fast.
    let config = Config::from_env().context("loading configuration from environment")?;

    // Initialize Sentry first so the panic hook and the tracing integration are
    // armed before we emit any logs. The guard must live for the whole program;
    // dropping it flushes and disables the client. `None` (no SENTRY_DSN) leaves
    // the tracing layer a no-op.
    let _sentry_guard = init_sentry(config.sentry.as_ref());

    init_tracing();

    tracing::info!(org = %config.organization_name, bind = %config.bind_addr, "starting soika");

    let pool = connect_and_migrate(&config.database_url)
        .await
        .context("connecting to database and running migrations")?;

    // When SSO is enabled, run OIDC discovery up front so a misconfigured issuer
    // fails the boot rather than the first login attempt (fail-fast).
    let oidc: Option<Arc<dyn OidcProvider>> = match &config.oidc {
        Some(oidc_config) => {
            tracing::info!(issuer = %oidc_config.issuer_url, "performing OIDC discovery");
            // Auto-provisioning trusts the IdP's verified email. With no domain
            // allow-list a public IdP can mint an account for any of its users.
            if oidc_config.allowed_email_domains.is_empty() {
                tracing::warn!(
                    "OAUTH_ALLOWED_EMAIL_DOMAINS is empty: any verified email from the \
                     provider may auto-provision an account; set it to restrict sign-ups"
                );
            }
            // GitHub is OAuth2-only (no OIDC discovery / ID-token); the generic
            // flavour discovers the issuer up front so a misconfig fails the boot.
            let provider: Arc<dyn OidcProvider> = match oidc_config.kind {
                OAuthProviderKind::Github => {
                    tracing::info!("configuring GitHub OAuth2 provider (no OIDC discovery)");
                    Arc::new(
                        GithubProvider::new(oidc_config)
                            .context("building GitHub OAuth2 provider")?,
                    )
                }
                OAuthProviderKind::Oidc => {
                    tracing::info!(issuer = %oidc_config.issuer_url, "performing OIDC discovery");
                    Arc::new(
                        OidcClient::discover(oidc_config)
                            .await
                            .context("OIDC discovery failed at startup")?,
                    )
                }
            };
            Some(provider)
        }
        None => None,
    };

    let state = build_state(pool, config.clone())
        .context("building application state")?
        .with_oidc(oidc);

    // The built-in admin is no longer provisioned from env: on first run the
    // service is uninitialized and the SPA routes the operator to `/setup`,
    // which creates the instance admin via `POST /auth/setup`.

    // Background scheduler (retention cleanup / mail) — same tokio runtime.
    let scheduler_handle = scheduler::spawn(state.clone());

    let app = soika::router::build(state);

    let listener = TcpListener::bind(&config.bind_addr)
        .await
        .with_context(|| format!("binding to {}", config.bind_addr))?;
    tracing::info!("listening on {}", config.bind_addr);

    // `into_make_service_with_connect_info` exposes the TCP peer address to
    // handlers via `ConnectInfo<SocketAddr>`, used as the brute-force fallback
    // key when no `X-Forwarded-For` / `X-Real-IP` proxy header is present.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("serving HTTP")?;

    scheduler_handle.abort();
    tracing::info!("shutdown complete");
    Ok(())
}

/// Initialize tracing with an env-filter (`RUST_LOG`), defaulting to `info`.
///
/// The Sentry layer is always attached: when Sentry is disabled (no
/// `SENTRY_DSN`) the current hub has no client and the layer is a no-op. With it
/// enabled, `error!` events become Sentry events and lower levels become
/// breadcrumbs.
fn init_tracing() {
    use sentry::integrations::tracing::{EventFilter, default_event_filter};

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,soika=debug"));
    let sentry_layer = sentry::integrations::tracing::layer().event_filter(|md| {
        // `tower-http`'s TraceLayer logs a generic `ERROR "response failed"` for
        // every 5xx. That carries no route or cause, so promoting it to its own
        // Sentry issue just collapses all server faults into one useless group.
        // We log the real cause ourselves in `ApiError::into_response`, so keep
        // the middleware line as a breadcrumb (context) rather than an event.
        if md.target().starts_with("tower_http::trace") {
            return EventFilter::Breadcrumb;
        }
        default_event_filter(md)
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(sentry_layer)
        .init();
}

/// Initialize the Sentry client when `SENTRY_DSN` is configured, returning the
/// guard that must be kept alive for the program's lifetime (dropping it flushes
/// pending events and disables the client). Returns `None` when Sentry is off.
///
/// Note: the release build uses `panic = "abort"`, so a panic event is captured
/// by the hook on a best-effort basis but may not flush before the process
/// aborts; `tracing::error!` events are reported reliably.
fn init_sentry(cfg: Option<&SentryConfig>) -> Option<sentry::ClientInitGuard> {
    let cfg = cfg?;
    let options = sentry::ClientOptions {
        release: sentry::release_name!(),
        environment: cfg.environment.clone().map(Into::into),
        // Don't attach a thread stack trace to message events: in the stripped
        // release binary it symbolicates to a wall of `<unknown>` frames, and
        // even with symbols it would be the async/tower poll stack, not where
        // the error originated. The logged message + request context carry the
        // diagnostically useful information instead.
        attach_stacktrace: false,
        ..Default::default()
    };
    Some(sentry::init((cfg.dsn.clone(), options)))
}

/// Connect to SQLite (creating the file if missing), then run migrations.
async fn connect_and_migrate(database_url: &str) -> Result<sqlx::SqlitePool> {
    // `create_if_missing` is the SQLite-specific bit; isolated here in the bin.
    let connect_options = SqliteConnectOptions::from_str(database_url)
        .context("parsing DATABASE_URL")?
        .create_if_missing(true)
        // Enforce FK constraints per connection so ON DELETE CASCADE fires for
        // sessions / team_members / invites (SQLite defaults off).
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .disable_statement_logging();

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(connect_options)
        .await
        .context("opening SQLite pool")?;

    MIGRATOR.run(&pool).await.context("running migrations")?;
    tracing::info!("database migrations applied");

    // Give pre-existing projects/issues a random short public id (migration 0007
    // adds the column but leaves old rows NULL). New rows are assigned one on
    // insert; this is a no-op once every row is filled. Count the rows still
    // missing one up front so the log line reports how many were backfilled
    // (the backfill itself returns no count).
    let pending_short_ids: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM projects WHERE short_id IS NULL) \
         + (SELECT COUNT(*) FROM issues WHERE short_id IS NULL)",
    )
    .fetch_one(&pool)
    .await
    .context("counting rows missing a short id")?;

    soika::adapters::sqlite::backfill_short_ids(&pool)
        .await
        .context("backfilling short ids")?;

    if pending_short_ids > 0 {
        tracing::info!(rows = pending_short_ids, "backfilled short ids");
    } else {
        tracing::debug!("short id backfill: nothing to do");
    }

    Ok(pool)
}

/// Resolve when Ctrl-C or SIGTERM is received.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
