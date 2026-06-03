//! Client (frontend) telemetry configuration.
//!
//! Serves the SPA the values it needs to initialize the Sentry browser SDK at
//! runtime. This endpoint is **session-authenticated** ([`AuthUser`]) on
//! purpose: the Sentry DSN must never appear on an unauthenticated route, so the
//! SPA only initializes error reporting after the operator has signed in.
//!
//! A single DSN is shared with the backend; the `release` mirrors the backend's
//! `sentry::release_name!()` (`soika@<version>`) so both halves report under the
//! same release.

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::api::teams::AuthUser;
use crate::state::AppState;

/// Client-safe telemetry config returned by `GET /api/client-config`.
///
/// All Sentry fields are `null` when error reporting is disabled (no
/// `SENTRY_DSN`), in which case the SPA skips SDK initialization entirely.
#[derive(Debug, Serialize)]
pub struct ClientConfigView {
    /// Sentry DSN the browser SDK reports to; `null` disables frontend Sentry.
    pub sentry_dsn: Option<String>,
    /// Optional environment tag (`SENTRY_ENVIRONMENT`); `null` leaves it unset.
    pub sentry_environment: Option<String>,
    /// Release identifier, matching the backend (`soika@<version>`).
    pub release: String,
}

/// `GET /api/client-config` — telemetry config for the SPA.
///
/// Requires a valid session (`AuthUser`); unauthenticated requests get `401`,
/// keeping the DSN off any public route.
pub async fn get(_user: AuthUser, State(state): State<AppState>) -> Json<ClientConfigView> {
    let (sentry_dsn, sentry_environment) = match &state.config.sentry {
        Some(sentry) => (Some(sentry.dsn.clone()), sentry.environment.clone()),
        None => (None, None),
    };

    Json(ClientConfigView {
        sentry_dsn,
        sentry_environment,
        release: concat!("soika@", env!("CARGO_PKG_VERSION")).to_string(),
    })
}
