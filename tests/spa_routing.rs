//! Regression tests for the SPA / API path-namespace split.
//!
//! The whole JSON API lives under `/api/*`; the root path namespace belongs to
//! the SPA. The bug this guards against: the internal API was mounted at the
//! root, so a *browser* navigation to `/teams/{id}` matched the JSON handler and
//! returned raw JSON instead of the app shell. These tests drive the real
//! router in-process and assert:
//!
//!   * a direct hit on an SPA client route (`/teams/{id}`, `/profile`,
//!     `/invite/{token}`) serves the embedded `index.html`, not JSON;
//!   * the JSON API is reachable under `/api/*` (and still enforces auth).

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use soika::{Config, MIGRATOR, build_state, router};
use tower::ServiceExt; // for `oneshot`

fn test_config() -> Config {
    Config {
        organization_name: "test".into(),
        database_url: "sqlite::memory:".into(),
        bind_addr: "127.0.0.1:0".into(),
        base_url: "http://localhost".into(),
        secret_key: "test-secret-key".into(),
        allow_signup: false,
        default_events_retention: 1000,
        default_retention_days: 0,
        retention_cron: "0 0 * * * *".into(),
        smtp: None,
        oidc: None,
        sentry: None,
        lockout: Default::default(),
    }
}

async fn build_router() -> axum::Router {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");
    MIGRATOR.run(&pool).await.expect("run migrations");
    let state = build_state(pool, test_config());
    router::build(state)
}

/// A GET request the way a browser navigation arrives (asking for HTML).
async fn get(router: &axum::Router, uri: &str) -> (StatusCode, String, String) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::ACCEPT, "text/html")
        .body(Body::empty())
        .expect("build request");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("router response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (
        status,
        content_type,
        String::from_utf8_lossy(&bytes).to_string(),
    )
}

/// The bug, exactly: navigating to a team page must serve the SPA shell, not the
/// JSON team record.
#[tokio::test]
async fn team_page_serves_spa_not_json() {
    let router = build_router().await;
    let (status, content_type, body) =
        get(&router, "/teams/8056877a-dd8f-4abe-bacc-528525c5edf2").await;

    assert_eq!(
        status,
        StatusCode::OK,
        "SPA fallback should serve the shell"
    );
    assert!(
        content_type.starts_with("text/html"),
        "expected HTML, got {content_type:?}"
    );
    assert!(
        body.contains("<html") || body.contains("<!doctype"),
        "body should be the app shell, not JSON: {body:.120}"
    );
    assert!(
        !body.contains("\"members\""),
        "must not leak the JSON team record"
    );
}

/// Other root-level client routes that previously collided with API handlers.
#[tokio::test]
async fn root_client_routes_serve_spa() {
    let router = build_router().await;
    for uri in ["/profile", "/teams", "/invite/some-token"] {
        let (status, content_type, _) = get(&router, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} should hit the SPA fallback");
        assert!(
            content_type.starts_with("text/html"),
            "{uri} should serve HTML, got {content_type:?}"
        );
    }
}

/// The JSON API moved under `/api/*` and still enforces session auth: an
/// unauthenticated request is rejected (proving the handler — not the SPA
/// fallback — is what answers there).
#[tokio::test]
async fn api_routes_live_under_api_prefix_and_require_auth() {
    let router = build_router().await;
    let (status, _, _) = get(&router, "/api/teams/8056877a-dd8f-4abe-bacc-528525c5edf2").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "GET /api/teams/{{id}} should require a session"
    );
}
