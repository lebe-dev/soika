//! Integration tests for the passkey (WebAuthn) HTTP surface.
//!
//! These drive the real router in-process against a fresh `:memory:` database.
//! A full ceremony cannot be replayed here — signing an assertion needs a real
//! authenticator — so the tests cover everything around it: the feature gate,
//! the signed challenge cookies, ownership of stored credentials, the per-user
//! quota, and the rejection paths a forged or replayed response takes.

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::auth::hash_password;
use soika::config::PasskeyConfig;
use soika::domain::{AuthProvider, Id, InstanceRole, UserStatus};
use soika::ports::{NewPasskey, NewUser};
use soika::{AppState, Config, MIGRATOR, build_state, router};
use std::time::Duration;
use tower::ServiceExt;

const SECRET: &str = "test-secret-key";
const PASSWORD: &str = "supersecret";

fn test_config(passkey: Option<PasskeyConfig>) -> Config {
    Config {
        organization_name: "test".into(),
        database_url: "sqlite::memory:".into(),
        db: soika::config::DbConfig::default(),
        bind_addr: "127.0.0.1:0".into(),
        base_url: "http://localhost".into(),
        secret_key: SECRET.into(),
        allow_signup: false,
        default_events_retention: 1000,
        default_retention_days: 0,
        retention_cron: "0 0 * * * *".into(),
        smtp: None,
        oidc: None,
        passkey,
        sentry: None,
        lockout: Default::default(),
        timezone: "UTC".to_string(),
    }
}

fn passkey_config() -> PasskeyConfig {
    PasskeyConfig {
        rp_id: "localhost".into(),
        rp_name: "soika".into(),
        rp_origin: "http://localhost".into(),
        extra_origins: Vec::new(),
        allow_subdomains: false,
        timeout: Duration::from_secs(60),
        challenge_ttl: Duration::from_secs(300),
        max_per_user: 2,
    }
}

/// Build a router with passkeys either enabled or disabled.
async fn build_app(enabled: bool) -> (axum::Router, AppState) {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");
    MIGRATOR.run(&pool).await.expect("run migrations");

    let passkey = enabled.then(passkey_config);
    let state = build_state(pool, test_config(passkey)).expect("build state");
    (router::build(state.clone()), state)
}

async fn seed_user(state: &AppState, email: &str, status: UserStatus) -> Id {
    state
        .users
        .create(NewUser {
            email: email.into(),
            display_name: "Seed User".into(),
            password_hash: hash_password(PASSWORD).expect("hash"),
            instance_role: InstanceRole::Member,
            auth_provider: AuthProvider::Local,
            status,
        })
        .await
        .expect("seed user")
        .id
}

/// Store a credential directly (the ceremony itself needs a real authenticator).
async fn seed_passkey(state: &AppState, user_id: Id, credential_id: &str, name: &str) -> Id {
    state
        .passkeys
        .create(NewPasskey {
            user_id,
            credential_id: credential_id.into(),
            name: name.into(),
            credential: json!({ "stub": true }).to_string(),
        })
        .await
        .expect("seed passkey")
        .id
}

async fn login_session_cookie(router: &axum::Router, email: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "email": email, "password": PASSWORD }).to_string(),
        ))
        .expect("build request");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("router response");
    assert_eq!(response.status(), StatusCode::OK, "login should succeed");
    let raw = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.starts_with("soika_session="))
        .expect("session cookie set on login");
    raw.split(';').next().expect("cookie key=value").to_string()
}

/// Issue a request and return (status, Set-Cookie values, parsed JSON body).
async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Vec<String>, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("build request");

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("router response");
    let status = response.status();
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_owned)
        .collect();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, cookies, json)
}

/// Turn a `Set-Cookie` list into a `Cookie` request header for `name`.
fn cookie_header(set_cookies: &[String], name: &str) -> String {
    let prefix = format!("{name}=");
    let raw = set_cookies
        .iter()
        .find(|c| c.starts_with(&prefix))
        .unwrap_or_else(|| panic!("{name} cookie should be set"));
    raw.split(';').next().expect("cookie key=value").to_string()
}

/// A syntactically valid but cryptographically bogus assertion.
fn fake_assertion() -> Value {
    json!({
        "id": "Y3JlZC1vbmU",
        "rawId": "Y3JlZC1vbmU",
        "response": {
            "authenticatorData": "AA",
            "clientDataJSON": "AA",
            "signature": "AA",
            "userHandle": "AA"
        },
        "type": "public-key",
        "extensions": {}
    })
}

// ---------------------------------------------------------------------------
// Feature gate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn routes_are_absent_when_passkeys_are_disabled() {
    let (router, state) = build_app(false).await;
    seed_user(&state, "user@example.com", UserStatus::Active).await;
    let session = login_session_cookie(&router, "user@example.com").await;

    let (status, _, _) = send(&router, "POST", "/auth/passkey/login/options", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = send(&router, "GET", "/api/passkeys", Some(&session), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = send(
        &router,
        "POST",
        "/api/passkeys/options",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn auth_config_advertises_the_feature_flag() {
    let (router, _) = build_app(true).await;
    let (status, _, body) = send(&router, "GET", "/auth/config", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["passkey_enabled"], json!(true));

    let (router, _) = build_app(false).await;
    let (_, _, body) = send(&router, "GET", "/auth/config", None, None).await;
    assert_eq!(body["passkey_enabled"], json!(false));
}

// ---------------------------------------------------------------------------
// Sign-in ceremony
// ---------------------------------------------------------------------------

#[tokio::test]
async fn login_options_issue_a_challenge_and_a_signed_cookie() {
    let (router, _) = build_app(true).await;

    let (status, cookies, body) =
        send(&router, "POST", "/auth/passkey/login/options", None, None).await;

    assert_eq!(status, StatusCode::OK);
    // Usernameless: no credential is pre-selected for the browser.
    assert!(body["publicKey"]["challenge"].is_string());
    assert_eq!(body["publicKey"]["userVerification"], json!("required"));
    assert!(body["mediation"].is_null(), "mediation must not be forced");

    let cookie = cookies
        .iter()
        .find(|c| c.starts_with("soika_passkey_auth="))
        .expect("challenge cookie");
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(cookie.contains("Max-Age=300"));
}

#[tokio::test]
async fn login_without_a_challenge_cookie_is_unauthorized() {
    let (router, _) = build_app(true).await;

    let (status, _, _) = send(
        &router,
        "POST",
        "/auth/passkey/login",
        None,
        Some(fake_assertion()),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_with_a_tampered_challenge_cookie_is_unauthorized() {
    let (router, _) = build_app(true).await;
    let (_, cookies, _) = send(&router, "POST", "/auth/passkey/login/options", None, None).await;
    let cookie = cookie_header(&cookies, "soika_passkey_auth");
    let tampered = format!("{cookie}tamper");

    let (status, _, _) = send(
        &router,
        "POST",
        "/auth/passkey/login",
        Some(&tampered),
        Some(fake_assertion()),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_with_an_unknown_credential_is_unauthorized() {
    let (router, _) = build_app(true).await;
    let (_, cookies, _) = send(&router, "POST", "/auth/passkey/login/options", None, None).await;
    let cookie = cookie_header(&cookies, "soika_passkey_auth");

    let (status, _, body) = send(
        &router,
        "POST",
        "/auth/passkey/login",
        Some(&cookie),
        Some(fake_assertion()),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // The response must not reveal whether the account or the credential exists.
    assert_eq!(
        body["error"],
        json!("authentication error: passkey is not recognised")
    );
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registration_requires_a_session() {
    let (router, _) = build_app(true).await;
    let (status, _, _) = send(&router, "POST", "/api/passkeys/options", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _, _) = send(&router, "GET", "/api/passkeys", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn registration_options_require_a_discoverable_credential() {
    let (router, state) = build_app(true).await;
    let user_id = seed_user(&state, "reg@example.com", UserStatus::Active).await;
    seed_passkey(&state, user_id, "ZXhpc3RpbmctY3JlZA", "Old key").await;
    let session = login_session_cookie(&router, "reg@example.com").await;

    let (status, cookies, body) = send(
        &router,
        "POST",
        "/api/passkeys/options",
        Some(&session),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let selection = &body["publicKey"]["authenticatorSelection"];
    assert_eq!(selection["residentKey"], json!("required"));
    assert_eq!(selection["requireResidentKey"], json!(true));
    assert_eq!(selection["userVerification"], json!("required"));
    // The user's existing credential is excluded so it cannot be enrolled twice.
    assert_eq!(
        body["publicKey"]["excludeCredentials"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let cookie = cookies
        .iter()
        .find(|c| c.starts_with("soika_passkey_reg="))
        .expect("registration challenge cookie");
    assert!(cookie.contains("HttpOnly"));
}

#[tokio::test]
async fn registration_is_refused_once_the_quota_is_reached() {
    let (router, state) = build_app(true).await;
    let user_id = seed_user(&state, "quota@example.com", UserStatus::Active).await;
    // `max_per_user` is 2 in the test config.
    seed_passkey(&state, user_id, "Y3JlZC1h", "A").await;
    seed_passkey(&state, user_id, "Y3JlZC1i", "B").await;
    let session = login_session_cookie(&router, "quota@example.com").await;

    let (status, _, body) = send(
        &router,
        "POST",
        "/api/passkeys/options",
        Some(&session),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap_or_default().contains("2"),
        "error should name the limit: {body}"
    );
}

#[tokio::test]
async fn registration_without_a_challenge_cookie_is_unauthorized() {
    let (router, state) = build_app(true).await;
    seed_user(&state, "nocookie@example.com", UserStatus::Active).await;
    let session = login_session_cookie(&router, "nocookie@example.com").await;

    let (status, _, _) = send(
        &router,
        "POST",
        "/api/passkeys",
        Some(&session),
        Some(json!({
            "name": "My key",
            "credential": {
                "id": "Y3JlZA",
                "rawId": "Y3JlZA",
                "response": { "attestationObject": "AA", "clientDataJSON": "AA" },
                "type": "public-key",
                "extensions": {}
            }
        })),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_registration_challenge_cannot_be_used_by_another_account() {
    let (router, state) = build_app(true).await;
    seed_user(&state, "first@example.com", UserStatus::Active).await;
    seed_user(&state, "second@example.com", UserStatus::Active).await;

    let first = login_session_cookie(&router, "first@example.com").await;
    let (_, cookies, _) = send(&router, "POST", "/api/passkeys/options", Some(&first), None).await;
    let challenge = cookie_header(&cookies, "soika_passkey_reg");

    // The second account replays the first account's challenge cookie.
    let second = login_session_cookie(&router, "second@example.com").await;
    let (status, _, _) = send(
        &router,
        "POST",
        "/api/passkeys",
        Some(&format!("{second}; {challenge}")),
        Some(json!({
            "name": "Stolen",
            "credential": {
                "id": "Y3JlZA",
                "rawId": "Y3JlZA",
                "response": { "attestationObject": "AA", "clientDataJSON": "AA" },
                "type": "public-key",
                "extensions": {}
            }
        })),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Management
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_returns_only_the_callers_credentials_without_key_material() {
    let (router, state) = build_app(true).await;
    let owner = seed_user(&state, "owner@example.com", UserStatus::Active).await;
    let other = seed_user(&state, "other@example.com", UserStatus::Active).await;
    seed_passkey(&state, owner, "Y3JlZC1vd25lcg", "MacBook").await;
    seed_passkey(&state, other, "Y3JlZC1vdGhlcg", "Phone").await;

    let session = login_session_cookie(&router, "owner@example.com").await;
    let (status, _, body) = send(&router, "GET", "/api/passkeys", Some(&session), None).await;

    assert_eq!(status, StatusCode::OK);
    let keys = body.as_array().expect("array of passkeys");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["name"], json!("MacBook"));
    assert!(
        keys[0]["credential"].is_null(),
        "no key material in the view"
    );
    assert!(keys[0]["last_used_at"].is_null());
}

#[tokio::test]
async fn rename_and_delete_are_scoped_to_the_owner() {
    let (router, state) = build_app(true).await;
    let owner = seed_user(&state, "owner@example.com", UserStatus::Active).await;
    seed_user(&state, "other@example.com", UserStatus::Active).await;
    let key_id = seed_passkey(&state, owner, "Y3JlZC1vd25lcg", "MacBook").await;

    let owner_session = login_session_cookie(&router, "owner@example.com").await;
    let (status, _, body) = send(
        &router,
        "PATCH",
        &format!("/api/passkeys/{key_id}"),
        Some(&owner_session),
        Some(json!({ "name": "  Work laptop  " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], json!("Work laptop"));

    let other_session = login_session_cookie(&router, "other@example.com").await;
    let (status, _, _) = send(
        &router,
        "DELETE",
        &format!("/api/passkeys/{key_id}"),
        Some(&other_session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = send(
        &router,
        "DELETE",
        &format!("/api/passkeys/{key_id}"),
        Some(&owner_session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, _, body) = send(&router, "GET", "/api/passkeys", Some(&owner_session), None).await;
    assert_eq!(body.as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn deleting_a_user_removes_their_passkeys() {
    let (_, state) = build_app(true).await;
    let user_id = seed_user(&state, "gone@example.com", UserStatus::Active).await;
    seed_passkey(&state, user_id, "Y3JlZC1nb25l", "Key").await;

    state.users.delete(user_id).await.expect("delete user");

    let found = state
        .passkeys
        .find_by_credential_id("Y3JlZC1nb25l")
        .await
        .expect("lookup");
    assert!(found.is_none(), "passkeys must not outlive their account");
}
