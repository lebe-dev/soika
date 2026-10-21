//! Locale persistence, validation and first-visit initialization through the real API.
mod common;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use soika::config::LockoutConfig;
use soika::{Config, build_state, router};
use std::time::Duration;
use tower::ServiceExt;

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    body: Value,
) -> (StatusCode, Value, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string())
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

#[tokio::test]
async fn locale_is_initialized_once_then_explicitly_changeable_and_persistent() {
    let pool = common::test_db().await;
    let state = build_state(pool, test_config()).unwrap();
    let app = router::build(state.clone());
    let (status, user, cookie) = send(&app, "POST", "/auth/setup", "", json!({
        "email": "owner@example.com", "password": "correct-horse-battery", "display_name": "Owner", "org_name": "Test"
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(user["locale"].is_null());
    let (status, _, _) = send(&app, "PATCH", "/api/profile", "", json!({"locale": "ru"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, user, _) = send(
        &app,
        "PATCH",
        "/api/profile",
        &cookie,
        json!({"initial_locale": "ru"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["locale"], "ru");
    let (_, user, _) = send(
        &app,
        "PATCH",
        "/api/profile",
        &cookie,
        json!({"initial_locale": "en"}),
    )
    .await;
    assert_eq!(
        user["locale"], "ru",
        "another browser must not overwrite a saved choice"
    );
    for value in [json!("de"), json!("ru-RU"), json!(12)] {
        let (status, _, _) = send(
            &app,
            "PATCH",
            "/api/profile",
            &cookie,
            json!({"locale": value}),
        )
        .await;
        assert!(status.is_client_error());
    }
    let (_, user, _) = send(
        &app,
        "PATCH",
        "/api/profile",
        &cookie,
        json!({"locale": "en"}),
    )
    .await;
    assert_eq!(user["locale"], "en");
    let (_, user, _) = send(
        &app,
        "PATCH",
        "/api/profile",
        &cookie,
        json!({"display_name": "Renamed"}),
    )
    .await;
    assert_eq!(user["locale"], "en");
    let (_, config, _) = send(&app, "GET", "/auth/config", &cookie, Value::Null).await;
    assert_eq!(config["user"]["locale"], "en");
    let (_, user, _) = send(
        &app,
        "POST",
        "/auth/login",
        "",
        json!({"email": "owner@example.com", "password": "correct-horse-battery"}),
    )
    .await;
    assert_eq!(user["locale"], "en");
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
        default_events_retention: 1000,
        default_retention_days: 0,
        retention_cron: "0 0 * * * *".into(),
        smtp: None,
        oidc: None,
        passkey: None,
        sentry: None,
        lockout: LockoutConfig {
            enabled: false,
            max_attempts: 100,
            window: Duration::from_secs(900),
            base_lockout: Duration::from_secs(60),
            max_lockout: Duration::from_secs(3600),
        },
        timezone: "UTC".to_string(),
    }
}
