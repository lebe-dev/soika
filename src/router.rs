//! Complete axum router wiring every route onto stub handlers.
//!
//! Owned by foundation: feature agents fill in handler BODIES, not this file.
//! Routes are grouped: auth (root), the JSON API under `/api/*` (Sentry-compatible
//! ingestion with DSN auth + the session-cookie internal API, including invites),
//! health, and the SPA fallback. The `/api` prefix keeps the root path namespace
//! free for SPA client routes so a direct hit on e.g. `/teams/{id}` serves the
//! app shell, not raw JSON.

use std::time::Duration;

use axum::Router;
use axum::http::{Method, Request, Response};
use axum::routing::{delete, get, patch, post};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

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
    // --- Ingestion (DSN auth, Sentry-compatible) ---
    // Kept inside the `/api` namespace so the public ingest URLs stay
    // `/api/{project_id}/envelope/` (and the legacy `/store/`) exactly as SDKs
    // expect. Browser SDKs post cross-origin, so these routes carry a permissive
    // CORS layer (any origin, POST + preflight OPTIONS) — auth is by DSN public
    // key, not origin, so `*` is safe. The layer is scoped to ingest only: the
    // cookie-authenticated JSON API is same-origin and must NOT use `Allow-Origin: *`
    // (incompatible with credentials). CorsLayer answers preflight itself.
    let ingest_cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers(Any);
    let ingest_routes = Router::new()
        .route("/:project_id/envelope/", post(ingest::envelope))
        // Legacy store endpoint: single bare JSON event (pre-envelope SDKs).
        .route("/:project_id/store/", post(ingest::store))
        .layer(ingest_cors);

    let api_routes = Router::new()
        .merge(ingest_routes)
        // --- Projects ---
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
            "/projects/:id/mute-rules",
            get(api::mute_rules::list).post(api::mute_rules::create),
        )
        .route(
            "/projects/:id/mute-rules/:rule_id",
            delete(api::mute_rules::delete),
        )
        .route("/projects/:id/favorite", post(api::projects::favorite))
        .route(
            "/projects/:id/regenerate-dsn",
            post(api::projects::regenerate_dsn),
        )
        // --- Issues ---
        .route("/issues/:id", get(api::issues::get))
        .route("/issues/:id/events", get(api::issues::list_events))
        .route("/issues/:id/resolve", post(api::issues::resolve))
        .route("/issues/:id/mute", post(api::issues::mute))
        .route("/issues/:id/unresolve", post(api::issues::unresolve))
        .route(
            "/issues/:id/fingerprint",
            patch(api::issues::update_fingerprint),
        )
        // --- Events ---
        .route("/events/:id", get(api::events::get))
        // --- Teams ---
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
            delete(api::teams::remove_member).patch(api::teams::set_member_role),
        )
        // --- Team invites ---
        .route(
            "/teams/:id/invites",
            get(api::invites::list).post(api::invites::create),
        )
        .route("/teams/:id/invites/:token", delete(api::invites::revoke))
        // --- Profile ---
        // Read goes through the bootstrap `/auth/config` (which embeds the
        // current user); only the update verb lives here.
        .route("/profile", patch(api::profile::update))
        // --- Invites (preview/accept). The browser-facing accept *page* is the
        // SPA route `/invite/{token}`; this is the JSON API it calls. ---
        .route(
            "/invite/:token",
            get(auth::get_invite).post(auth::accept_invite),
        )
        // --- Service settings & admin ---
        .route(
            "/settings",
            get(api::settings::get).patch(api::settings::update),
        )
        .route("/settings/test-email", post(api::settings::test_email))
        .route("/admin/users", get(api::settings::list_users))
        .route(
            "/admin/users/:id/approve",
            post(api::settings::approve_user),
        )
        .route(
            "/admin/users/:id",
            patch(api::settings::set_user_role).delete(api::settings::delete_user),
        );

    let auth_routes = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/register", post(auth::register))
        // First-run admin provisioning; rejected once initialized.
        .route("/auth/setup", post(auth::setup))
        // OIDC / SSO — browser navigations, not JSON.
        .route("/auth/oidc/login", get(auth::oidc_login))
        .route("/auth/oidc/callback", get(auth::oidc_callback))
        .route("/auth/config", get(auth::auth_config));

    Router::new()
        // Health
        .route("/healthz", get(healthz))
        // Auth lives at the root (no SPA page collides with `/auth/*`, and the
        // OIDC redirect URL is a stable, externally-registered path).
        .merge(auth_routes)
        // The entire JSON API (including Sentry-compatible ingestion) lives under
        // `/api/*`, keeping the root path namespace free for SPA client routes
        // (`/teams/{id}`, `/projects/{id}`, …) which the fallback serves.
        .nest("/api", api_routes)
        // SPA + embedded static assets — fallback last.
        .fallback(web::spa_fallback)
        .layer(CompressionLayer::new())
        // Per-request span for correlation: every log emitted while handling a
        // request inherits `request_id` (a generated UUID), `method`, and `path`.
        // The default `TraceLayer` emits nothing at the `info` filter, so we add
        // an explicit `on_response` event carrying status + latency.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &Request<_>| {
                    tracing::info_span!(
                        "http",
                        method = %req.method(),
                        path = %req.uri().path(),
                        request_id = %Uuid::new_v4(),
                    )
                })
                .on_response(
                    |res: &Response<_>, latency: Duration, _span: &tracing::Span| {
                        tracing::info!(
                            status = %res.status(),
                            latency_ms = latency.as_millis(),
                            "request completed"
                        );
                    },
                ),
        )
        .with_state(state)
}
