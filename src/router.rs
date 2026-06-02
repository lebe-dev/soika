//! Complete axum router wiring every route (MVP §16) onto stub handlers.
//!
//! Owned by foundation: feature agents fill in handler BODIES, not this file.
//! Routes are grouped: ingestion (DSN auth), auth/invite, internal JSON API
//! (session-cookie auth), health, and the SPA fallback.

use axum::Router;
use axum::routing::{delete, get, patch, post};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::api;
use crate::auth;
use crate::ingest;
use crate::state::AppState;
use crate::web;

/// `GET /healthz` — liveness probe.
async fn healthz() -> &'static str {
    "ok"
}

/// Build the full application router.
pub fn build(state: AppState) -> Router {
    let api_routes = Router::new()
        // --- Projects (§16) ---
        .route(
            "/projects",
            get(api::projects::list).post(api::projects::create),
        )
        .route(
            "/projects/:id",
            get(api::projects::get)
                .patch(api::projects::update)
                .delete(api::projects::delete),
        )
        .route("/projects/:id/issues", get(api::projects::list_issues))
        .route("/projects/:id/dsn", get(api::projects::dsn))
        .route("/projects/:id/sdk-setup", get(api::projects::sdk_setup))
        .route("/projects/:id/mute", post(api::projects::mute))
        .route(
            "/projects/:id/regenerate-dsn",
            post(api::projects::regenerate_dsn),
        )
        // --- Project members (§16, §10.2) ---
        .route("/projects/:id/members", get(api::members::list))
        .route(
            "/projects/:id/members/:user_id",
            delete(api::members::remove),
        )
        // --- Project invites (§9) ---
        .route(
            "/projects/:id/invites",
            get(api::invites::list).post(api::invites::create),
        )
        .route("/projects/:id/invites/:token", delete(api::invites::revoke))
        // --- Issues (§8.1) ---
        .route("/issues/:id", get(api::issues::get))
        .route("/issues/:id/events", get(api::issues::list_events))
        .route("/issues/:id/resolve", post(api::issues::resolve))
        .route("/issues/:id/mute", post(api::issues::mute))
        .route("/issues/:id/unresolve", post(api::issues::unresolve))
        .route(
            "/issues/:id/fingerprint",
            patch(api::issues::update_fingerprint),
        )
        // --- Events (§6) ---
        .route("/events/:id", get(api::events::get))
        // --- Teams (§4.1) ---
        .route("/teams", get(api::teams::list).post(api::teams::create))
        .route(
            "/teams/:id",
            get(api::teams::get)
                .patch(api::teams::update)
                .delete(api::teams::delete),
        )
        .route("/teams/:id/members", post(api::teams::add_member))
        .route(
            "/teams/:id/members/:user_id",
            delete(api::teams::remove_member),
        )
        // --- Profile (§10.3) ---
        .route(
            "/profile",
            get(api::profile::get).patch(api::profile::update),
        )
        // --- Service settings & admin (§11, §14) ---
        .route(
            "/settings",
            get(api::settings::get).patch(api::settings::update),
        )
        .route("/admin/users", get(api::settings::list_users));

    let auth_routes = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/register", post(auth::register))
        .route(
            "/invite/:token",
            get(auth::get_invite).post(auth::accept_invite),
        );

    Router::new()
        // Health
        .route("/healthz", get(healthz))
        // Ingestion (DSN auth, Sentry-compatible — §5.1)
        .route("/api/:project_id/envelope/", post(ingest::envelope))
        // Legacy store endpoint: single bare JSON event (pre-envelope SDKs).
        .route("/api/:project_id/store/", post(ingest::store))
        // Internal API + auth
        .merge(auth_routes)
        .merge(api_routes)
        // SPA + embedded static assets (§2.2) — fallback last.
        .fallback(web::spa_fallback)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
