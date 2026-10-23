//! Log safety and correlation at the real HTTP boundary.
mod common;

use axum::body::Body;
use axum::http::Request;
use soika::{Config, build_state, router};
use std::io::Write;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn test_config() -> Config {
    Config {
        organization_name: "test".into(),
        database_url: "sqlite::memory:".into(),
        db: soika::config::DbConfig::default(),
        bind_addr: "127.0.0.1:0".into(),
        base_url: "http://localhost".into(),
        secret_key: "test-secret-key".into(),
        allow_signup: false,
        admin_enabled: true,
        admin_users: Default::default(),
        default_events_retention: 1000,
        default_retention_days: 0,
        retention_cron: "0 0 * * * *".into(),
        webhooks_enabled: false,
        smtp_enabled: false,
        smtp: None,
        oidc: None,
        passkey: None,
        sentry: None,
        lockout: Default::default(),
        timezone: "UTC".to_string(),
    }
}

async fn build_router() -> axum::Router {
    let pool = common::test_db().await;
    let state = build_state(pool, test_config()).unwrap();
    router::build(state)
}

#[tokio::test]
async fn http_logs_redact_paths_queries_headers_and_correlate_boundaries() {
    let app = build_router().await;
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    let dispatch = tracing::Dispatch::new(subscriber);

    for uri in [
        "/api/invite/secret-invite-token?token=secret-query",
        "/invite/secret-spa-token?code=secret-code",
        "/unknown/secret-fallback-path",
    ] {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("authorization", "Bearer secret-header")
                    .header("cookie", "session=secret-cookie")
                    .body(Body::empty())
                    .unwrap(),
            )
            .with_subscriber(dispatch.clone())
            .await
            .unwrap();
    }

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(
        !logs.contains("secret-"),
        "sensitive request data in logs: {logs}"
    );
    assert!(
        logs.contains("/api/invite/{token}"),
        "missing route template: {logs}"
    );
    for line in logs
        .lines()
        .filter(|line| line.contains("request received") || line.contains("request completed"))
    {
        assert!(line.contains("request_id="), "missing correlation: {line}");
    }
    assert_eq!(logs.matches("request received").count(), 3, "{logs}");
    assert_eq!(logs.matches("request completed").count(), 3, "{logs}");
    let ids: Vec<_> = logs
        .lines()
        .filter(|line| line.contains("request received") || line.contains("request completed"))
        .map(|line| {
            line.split("request_id=")
                .nth(1)
                .unwrap()
                .split('}')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect();
    for pair in ids.chunks_exact(2) {
        assert_eq!(pair[0], pair[1]);
    }
    assert_ne!(ids[0], ids[2]);
}

#[test]
fn project_api_faults_are_logged_and_auth_contention_is_handled() {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use soika::Error;
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let response =
            soika::api::projects::error_response(Error::internal("repository unavailable"));
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let busy = Error::db_busy("create session", Error::Db(sqlx::Error::PoolClosed));
        let response = soika::auth::handlers::AuthError(busy).into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    });
    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert_eq!(logs.matches("ERROR").count(), 1, "{logs}");
    assert!(logs.contains("repository unavailable"), "{logs}");
    assert_eq!(logs.matches("WARN").count(), 1, "{logs}");
}
