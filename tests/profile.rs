//! Integration tests for `PATCH /api/profile` display-name validation.
//!
//! Drives the real application router in-process against a fresh `:memory:`
//! SQLite DB. A display name shorter than the minimum (after trimming) is
//! refused with `400` and leaves the stored name untouched; a valid one is
//! trimmed and saved.

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::auth::hash_password;
use soika::config::LockoutConfig;
use soika::domain::{AuthProvider, InstanceRole, UserStatus};
use soika::ports::NewUser;
use soika::{AppState, Config, build_state, router};
use std::time::Duration;
use tower::ServiceExt;

const EMAIL: &str = "jane@example.com";
const PASSWORD: &str = "correct-horse-battery";

struct Fixture {
    router: Router,
    state: AppState,
}

impl Fixture {
    async fn spawn() -> Fixture {
        Self::spawn_with(AuthProvider::Local, false).await
    }

    async fn spawn_with(provider: AuthProvider, allow_name_change: bool) -> Fixture {
        let pool = common::test_db().await;
        let mut config = test_config();
        config.sso_allow_display_name_change = allow_name_change;
        let state = build_state(pool, config).unwrap();
        state
            .users
            .create(NewUser {
                email: EMAIL.into(),
                display_name: "Jane".into(),
                password_hash: hash_password(PASSWORD).unwrap(),
                instance_role: InstanceRole::Member,
                auth_provider: provider,
                status: UserStatus::Active,
            })
            .await
            .unwrap();
        Fixture {
            router: router::build(state.clone()),
            state,
        }
    }

    /// Log in via `/auth/login` and return the session cookie pair to replay.
    async fn login(&self) -> String {
        let body = json!({ "email": EMAIL, "password": PASSWORD }).to_string();
        let request = Request::builder()
            .header("x-soika-request", "1")
            .method("POST")
            .uri("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "login should succeed");
        let set_cookie = response
            .headers()
            .get("set-cookie")
            .expect("login sets a session cookie")
            .to_str()
            .unwrap();
        set_cookie
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string()
    }

    async fn patch_profile(&self, cookie: &str, body: &Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .header("x-soika-request", "1")
            .method("PATCH")
            .uri("/api/profile")
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn stored_display_name(&self) -> String {
        self.state
            .users
            .find_by_email(EMAIL)
            .await
            .unwrap()
            .expect("user exists")
            .display_name
    }
}

#[tokio::test]
async fn too_short_display_name_is_rejected() {
    let fx = Fixture::spawn().await;
    let cookie = fx.login().await;

    for name in [".", "  ab  ", "   "] {
        let (status, body) = fx
            .patch_profile(&cookie, &json!({ "display_name": name }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name:?} must be refused");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|e| e.contains("at least 3 characters")),
            "error explains the rule, got {body}"
        );
    }

    assert_eq!(fx.stored_display_name().await, "Jane");
}

#[tokio::test]
async fn valid_display_name_is_trimmed_and_saved() {
    let fx = Fixture::spawn().await;
    let cookie = fx.login().await;

    let (status, body) = fx
        .patch_profile(&cookie, &json!({ "display_name": "  Jane Doe " }))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], "Jane Doe");
    assert_eq!(fx.stored_display_name().await, "Jane Doe");
}

#[tokio::test]
async fn sso_display_name_policy() {
    for (provider, allowed, expected) in [
        (AuthProvider::Oidc, false, StatusCode::FORBIDDEN),
        (AuthProvider::Oidc, true, StatusCode::OK),
        (AuthProvider::Local, false, StatusCode::OK),
        (AuthProvider::Local, true, StatusCode::OK),
    ] {
        let fx = Fixture::spawn_with(provider, allowed).await;
        let user = fx.state.users.find_by_email(EMAIL).await.unwrap().unwrap();
        let (_, cookie) = soika::auth::session::start_session(
            fx.state.sessions.as_ref(),
            fx.state.clock.as_ref(),
            &fx.state.config,
            user.id,
        )
        .await
        .unwrap();
        let cookie = cookie.split(';').next().unwrap();
        let response = fx
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .header("x-soika-request", "1")
                    .uri("/auth/config")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let config: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(config["sso_allow_display_name_change"], allowed);
        let (status, _) = fx
            .patch_profile(
                cookie,
                &json!({ "display_name": "Jane Doe", "notifications_enabled": false }),
            )
            .await;
        assert_eq!(status, expected);
        assert_eq!(
            fx.stored_display_name().await,
            if expected == StatusCode::OK {
                "Jane Doe"
            } else {
                "Jane"
            }
        );
        let stored = fx.state.users.find_by_id(user.id).await.unwrap().unwrap();
        assert_eq!(stored.notifications_enabled, expected != StatusCode::OK);
        // Other profile preferences remain editable even when name changes are forbidden.
        let (status, _) = fx.patch_profile(cookie, &json!({ "locale": "ru" })).await;
        assert_eq!(status, StatusCode::OK);
    }
}

fn test_config() -> Config {
    Config {
        management: Default::default(),
        organization_name: "test".into(),
        database_url: "sqlite::memory:".into(),
        db: soika::config::DbConfig::default(),
        bind_addr: "127.0.0.1:0".into(),
        base_url: "http://localhost".into(),
        secret_key: "test-secret-key".into(),
        allow_signup: false,
        admin_enabled: true,
        email_login_enabled: true,
        sso_allow_display_name_change: false,
        admin_users: Default::default(),
        default_events_retention: 1000,
        default_retention_days: 0,
        retention_cron: "0 0 * * * *".into(),
        notifications_alertmanager_enabled: false,
        alertmanager_url: None,
        webhooks_enabled: false,
        smtp_enabled: false,
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
