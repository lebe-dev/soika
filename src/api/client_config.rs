//! Client (frontend) telemetry configuration.
//!
//! Serves the SPA the values it needs to initialize the Sentry browser SDK at
//! runtime. The DSN must never appear on an unauthenticated route, so it is only
//! ever embedded for a signed-in caller: the bootstrap [`crate::auth::auth_config`]
//! (`GET /auth/config`) folds this payload in for an authenticated session, and
//! the SPA initializes error reporting from it after sign-in.
//!
//! A single DSN is shared with the backend; the `release` mirrors the backend's
//! `sentry::release_name!()` (`soika@<version>`) so both halves report under the
//! same release.

use serde::Serialize;

use crate::state::AppState;

/// Client-safe telemetry config embedded in the bootstrap `/auth/config`.
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

impl ClientConfigView {
    /// Build the telemetry view from the running config. Sentry fields are
    /// `None` when error reporting is disabled (no `SENTRY_DSN`).
    ///
    /// Only ever served to an authenticated session (via the bootstrap
    /// `/auth/config`), keeping the DSN off any public route.
    pub fn from_state(state: &AppState) -> Self {
        let (sentry_dsn, sentry_environment) = match &state.config.sentry {
            Some(sentry) => (Some(sentry.dsn.clone()), sentry.environment.clone()),
            None => (None, None),
        };

        ClientConfigView {
            sentry_dsn,
            sentry_environment,
            release: concat!("soika@", env!("CARGO_PKG_VERSION")).to_string(),
        }
    }
}
