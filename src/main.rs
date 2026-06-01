//! soika binary — wiring, config, axum bootstrap, graceful shutdown (MVP §2.1).
//!
//! The bin crate uses `anyhow` for error handling.

use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::ConnectOptions;
use tokio::net::TcpListener;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use soika::{auth, build_state, scheduler, Config, MIGRATOR};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = Config::from_env().context("loading configuration from environment")?;
    tracing::info!(org = %config.organization_name, bind = %config.bind_addr, "starting soika");

    let pool = connect_and_migrate(&config.database_url)
        .await
        .context("connecting to database and running migrations")?;

    let state = build_state(pool, config.clone());

    // Idempotently provision the built-in admin from ADMIN_EMAIL/ADMIN_PASSWORD (§11).
    let bootstrap = auth::bootstrap_admin(&*state.users, &config)
        .await
        .context("provisioning built-in admin account")?;
    tracing::info!(?bootstrap, "built-in admin bootstrap");

    // Background scheduler (retention cleanup / mail) — same tokio runtime.
    let scheduler_handle = scheduler::spawn(state.clone());

    let app = soika::router::build(state);

    let listener = TcpListener::bind(&config.bind_addr)
        .await
        .with_context(|| format!("binding to {}", config.bind_addr))?;
    tracing::info!("listening on {}", config.bind_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serving HTTP")?;

    scheduler_handle.abort();
    tracing::info!("shutdown complete");
    Ok(())
}

/// Initialize tracing with an env-filter (`RUST_LOG`), defaulting to `info`.
fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,soika=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Connect to SQLite (creating the file if missing), then run migrations.
async fn connect_and_migrate(database_url: &str) -> Result<sqlx::SqlitePool> {
    // `create_if_missing` is the SQLite-specific bit; isolated here in the bin.
    let connect_options = SqliteConnectOptions::from_str(database_url)
        .context("parsing DATABASE_URL")?
        .create_if_missing(true)
        // Enforce FK constraints per connection so ON DELETE CASCADE fires for
        // sessions / memberships / team_members / invites (SQLite defaults off).
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .disable_statement_logging();

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(connect_options)
        .await
        .context("opening SQLite pool")?;

    MIGRATOR.run(&pool).await.context("running migrations")?;
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
