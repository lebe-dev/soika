//! Integration tests for login brute-force protection.
//!
//! These drive the *real* application router in-process via
//! [`tower::ServiceExt::oneshot`] — no network, no containers. Each test builds
//! a fresh `:memory:` SQLite database, runs the migrations, seeds a known
//! password account, then POSTs at `/auth/login` to prove the per-(IP, email)
//! lockout the handler consults end-to-end: a burst of bad passwords trips a
//! `429` with a `Retry-After`, a success clears the counter, and the key is
//! scoped to the (client IP, email) pair.
//!
//! The client IP is taken from `X-Forwarded-For`; with no such header (and no
//! `ConnectInfo`, since `oneshot` doesn't attach one) the guard keys on
//! `"unknown"`, so tests that don't set the header all share one IP bucket.

use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::auth::hash_password;
use soika::config::LockoutConfig;
use soika::domain::{AuthProvider, UserStatus};
use soika::ports::{NewProject, NewUser};
use soika::{AppState, Config, MIGRATOR, build_state, router};
use tower::ServiceExt; // for `oneshot`

const EMAIL: &str = "alice@example.com";
const PASSWORD: &str = "correct-horse-battery";

/// A running application under test plus its wired state.
struct TestApp {
    router: Router,
}

impl TestApp {
    /// Build a fresh in-memory app whose login guard uses `lockout`, seeded with
    /// one known password account (`EMAIL` / `PASSWORD`).
    async fn spawn(lockout: LockoutConfig) -> TestApp {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        MIGRATOR.run(&pool).await.expect("run migrations");

        let state = build_state(pool, test_config(lockout));
        seed_account(&state).await;

        TestApp {
            router: router::build(state),
        }
    }

    /// POST `/auth/login` with the given credentials and optional client IP
    /// (sent as `X-Forwarded-For`). Returns status, `Retry-After`, and the JSON.
    async fn login(
        &self,
        email: &str,
        password: &str,
        forwarded_for: Option<&str>,
    ) -> (StatusCode, Option<String>, Value) {
        let body = json!({ "email": email, "password": password }).to_string();
        let mut builder = Request::builder()
            .method("POST")
            .uri("/auth/login")
            .header("content-type", "application/json");
        if let Some(ip) = forwarded_for {
            builder = builder.header("x-forwarded-for", ip);
        }
        let request = builder.body(Body::from(body)).expect("build request");

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router response");
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, retry_after, json)
    }
}

/// Seed the single known password account used across the tests.
async fn seed_account(state: &AppState) {
    state
        .users
        .create(NewUser {
            email: EMAIL.into(),
            display_name: "Alice".into(),
            password_hash: hash_password(PASSWORD).expect("hash password"),
            is_admin: false,
            auth_provider: AuthProvider::Local,
            status: UserStatus::Active,
        })
        .await
        .expect("create user");

    // A team + project so the instance isn't "uninitialized"; not strictly
    // required for login, but keeps the fixture close to a real deployment.
    let team = state
        .teams
        .create("test-team".into())
        .await
        .expect("create team");
    state
        .projects
        .create(NewProject {
            team_id: team.id,
            name: "Test Project".into(),
            slug: "test-project".into(),
            dsn_public_key: "test-public-key".into(),
            retention_events: 1000,
            retention_days: 0,
            webhook_url: None,
        })
        .await
        .expect("create project");
}

/// A lockout config with a short threshold; long base/window so the test clock
/// never rolls the window or expires the lock mid-run.
fn lockout_config(enabled: bool, max_attempts: u32) -> LockoutConfig {
    LockoutConfig {
        enabled,
        max_attempts,
        window: Duration::from_secs(900),
        base_lockout: Duration::from_secs(60),
        max_lockout: Duration::from_secs(3600),
    }
}

/// A minimal config: in-memory db, no SMTP, no SSO, signup disabled.
fn test_config(lockout: LockoutConfig) -> Config {
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
        lockout,
    }
}

#[tokio::test]
async fn bad_passwords_lock_after_threshold_with_retry_after() {
    let app = TestApp::spawn(lockout_config(true, 3)).await;

    // The first three bad passwords are rejected as plain auth failures, with
    // no Retry-After until the lock trips.
    for _ in 0..3 {
        let (status, retry_after, _) = app.login(EMAIL, "wrong", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(retry_after.is_none());
    }

    // The fourth attempt is now locked out: 429 with a positive Retry-After.
    let (status, retry_after, body) = app.login(EMAIL, "wrong", None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let secs: u64 = retry_after
        .expect("Retry-After present when locked")
        .parse()
        .expect("Retry-After is an integer");
    assert!(secs >= 1, "Retry-After must be non-zero, got {secs}");
    assert!(body.get("error").is_some(), "error body present");
}

#[tokio::test]
async fn locked_key_rejects_even_the_correct_password() {
    let app = TestApp::spawn(lockout_config(true, 2)).await;

    // Trip the lock with two bad passwords.
    for _ in 0..2 {
        let (status, _, _) = app.login(EMAIL, "wrong", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // Even the *correct* password is rejected while locked, proving the guard is
    // checked before the credential verify (so a lock can't be bypassed and the
    // hash isn't even computed).
    let (status, _, _) = app.login(EMAIL, PASSWORD, None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn successful_login_resets_the_counter() {
    let app = TestApp::spawn(lockout_config(true, 3)).await;

    // Two failures (still under the threshold of 3).
    for _ in 0..2 {
        let (status, _, _) = app.login(EMAIL, "wrong", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // A correct login succeeds and clears the accrued failures.
    let (status, _, _) = app.login(EMAIL, PASSWORD, None).await;
    assert_eq!(status, StatusCode::OK);

    // Two more failures are again only 401 — if the counter hadn't reset, the
    // second of these (the 4th lifetime failure) would already be locked.
    for _ in 0..2 {
        let (status, _, _) = app.login(EMAIL, "wrong", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn lockout_is_scoped_to_ip_and_email() {
    let app = TestApp::spawn(lockout_config(true, 2)).await;

    // Lock (1.1.1.1, alice) with two failures, confirmed by a 429 on the third.
    for _ in 0..2 {
        let (status, _, _) = app.login(EMAIL, "wrong", Some("1.1.1.1")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, _, _) = app.login(EMAIL, "wrong", Some("1.1.1.1")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    // The same account from a different IP is unaffected (keyed separately).
    let (status, _, _) = app.login(EMAIL, "wrong", Some("2.2.2.2")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A different account from the locked IP is likewise unaffected.
    let (status, _, _) = app.login("bob@example.com", "wrong", Some("1.1.1.1")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn disabled_guard_never_locks() {
    // Threshold of 1, but the guard is disabled: failures never escalate.
    let app = TestApp::spawn(lockout_config(false, 1)).await;

    for _ in 0..5 {
        let (status, retry_after, _) = app.login(EMAIL, "wrong", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(retry_after.is_none());
    }
}
