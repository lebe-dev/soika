//! Integration tests for the OIDC / SSO HTTP surface (PLAN §6.3, §6.5, §9).
//!
//! Like `tests/ingest.rs`, these drive the *real* application router in-process
//! via [`tower::ServiceExt::oneshot`] against a fresh `:memory:` database. The
//! provider boundary is faked through the [`OidcProvider`] trait so the
//! happy-path callback runs without any network or real ID-token signing.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use openidconnect::url::Url;
use openidconnect::{CsrfToken, Nonce, PkceCodeChallenge, PkceCodeVerifier};
use serde_json::Value;
use soika::auth::{AuthorizeRequest, OidcClaims, OidcProvider, hash_password};
use soika::domain::AuthProvider;
use soika::ports::NewUser;
use soika::{AppState, Config, MIGRATOR, build_state, router};
use tower::ServiceExt;

const SECRET: &str = "test-secret-key";

/// A scripted [`OidcProvider`] fake: `authorize_url` returns a fixed CSRF state
/// (so the callback test can echo it back), and `exchange_code` yields canned
/// claims regardless of the code.
struct FakeProvider {
    claims: OidcClaims,
    allowed_domains: Vec<String>,
    csrf_state: String,
}

impl FakeProvider {
    fn new(claims: OidcClaims) -> Self {
        FakeProvider {
            claims,
            allowed_domains: vec![],
            csrf_state: "fixed-csrf-state".into(),
        }
    }
}

#[async_trait]
impl OidcProvider for FakeProvider {
    fn authorize_url(&self) -> AuthorizeRequest {
        // PKCE verifier/challenge pair; the challenge value is irrelevant to the
        // fake exchange but the verifier round-trips through the state cookie.
        let (_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        AuthorizeRequest {
            url: Url::parse("https://idp.example.com/oauth/authorize?response_type=code").unwrap(),
            csrf_state: CsrfToken::new(self.csrf_state.clone()),
            nonce: Nonce::new("fixed-nonce".into()),
            pkce_verifier,
        }
    }

    async fn exchange_code(
        &self,
        _code: String,
        _pkce_verifier: PkceCodeVerifier,
        _nonce: Nonce,
    ) -> soika::Result<OidcClaims> {
        Ok(self.claims.clone())
    }

    fn provider_name(&self) -> &str {
        "GitLab"
    }

    fn allowed_email_domains(&self) -> &[String] {
        &self.allowed_domains
    }
}

/// Build a router with SSO either enabled (Some provider) or disabled (None).
async fn build_app(oidc: Option<Arc<dyn OidcProvider>>) -> (axum::Router, AppState) {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");
    MIGRATOR.run(&pool).await.expect("run migrations");

    let state = build_state(pool, test_config()).with_oidc(oidc);
    (router::build(state.clone()), state)
}

fn test_config() -> Config {
    Config {
        organization_name: "test".into(),
        database_url: "sqlite::memory:".into(),
        bind_addr: "127.0.0.1:0".into(),
        base_url: "http://localhost".into(),
        secret_key: SECRET.into(),
        allow_signup: false,
        admin_email: None,
        admin_password: None,
        default_events_retention: 1000,
        default_retention_days: 0,
        retention_cron: "0 0 * * * *".into(),
        smtp: None,
        oidc: None,
    }
}

fn verified_claims(email: &str) -> OidcClaims {
    OidcClaims {
        email: email.into(),
        email_verified: true,
        display_name: "Test User".into(),
    }
}

/// Issue a GET and return (status, Location header, all Set-Cookie values).
async fn get(router: &axum::Router, uri: &str, cookie: Option<&str>) -> (StatusCode, Option<String>, Vec<String>) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    let request = builder.body(Body::empty()).expect("build request");
    let response = router.clone().oneshot(request).await.expect("router response");
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_owned)
        .collect();
    (status, location, cookies)
}

/// POST a JSON body and return (status, parsed JSON body).
async fn post_json(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("build request");
    let response = router.clone().oneshot(request).await.expect("router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("read body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Seed a user directly through the repository (bypassing HTTP).
async fn seed_user(state: &AppState, email: &str, password: &str, is_admin: bool) {
    state
        .users
        .create(NewUser {
            email: email.into(),
            display_name: "Seed".into(),
            password_hash: hash_password(password).expect("hash"),
            is_admin,
            auth_provider: AuthProvider::Local,
        })
        .await
        .expect("seed user");
}

async fn get_json(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("build request");
    let response = router.clone().oneshot(request).await.expect("router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("read body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Extract the `soika_oidc_state` cookie value from a Set-Cookie list and
/// reformat it as a `Cookie` request header.
fn state_cookie_header(set_cookies: &[String]) -> String {
    let raw = set_cookies
        .iter()
        .find(|c| c.starts_with("soika_oidc_state="))
        .expect("state cookie present");
    let kv = raw.split(';').next().expect("cookie key=value");
    kv.to_string()
}

// --- /auth/oidc/login ------------------------------------------------------

#[tokio::test]
async fn login_redirects_to_provider_with_state_cookie() {
    let (router, _state) = build_app(Some(Arc::new(FakeProvider::new(verified_claims(
        "u@example.com",
    )))))
    .await;

    let (status, location, cookies) = get(&router, "/auth/oidc/login", None).await;

    assert_eq!(status, StatusCode::FOUND);
    let location = location.expect("Location header present");
    assert!(
        location.starts_with("https://idp.example.com/oauth/authorize"),
        "got {location}"
    );
    assert!(
        cookies.iter().any(|c| c.starts_with("soika_oidc_state=")),
        "state cookie set: {cookies:?}"
    );
    // Transient cookie hygiene (PLAN §6.2).
    let state = cookies
        .iter()
        .find(|c| c.starts_with("soika_oidc_state="))
        .unwrap();
    assert!(state.contains("HttpOnly"));
    assert!(state.contains("SameSite=Lax"));
}

#[tokio::test]
async fn login_when_disabled_is_not_found() {
    let (router, _state) = build_app(None).await;

    let (status, _location, _cookies) = get(&router, "/auth/oidc/login", None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- /auth/config ----------------------------------------------------------

#[tokio::test]
async fn auth_config_reflects_enabled_mode() {
    let (router, _state) = build_app(Some(Arc::new(FakeProvider::new(verified_claims(
        "u@example.com",
    )))))
    .await;

    let (status, json) = get_json(&router, "/auth/config").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["oauth_enabled"], true);
    assert_eq!(json["oauth_provider_name"], "GitLab");
    assert_eq!(json["password_login_enabled"], false);
}

#[tokio::test]
async fn auth_config_reflects_disabled_mode() {
    let (router, _state) = build_app(None).await;

    let (status, json) = get_json(&router, "/auth/config").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["oauth_enabled"], false);
    assert_eq!(json["oauth_provider_name"], "");
    assert_eq!(json["password_login_enabled"], true);
    assert_eq!(json["allow_signup"], false);
}

// --- /auth/oidc/callback ---------------------------------------------------

#[tokio::test]
async fn callback_happy_path_provisions_user_and_sets_session() {
    let email = "newcomer@example.com";
    let (router, state) = build_app(Some(Arc::new(FakeProvider::new(verified_claims(email))))).await;

    // 1) /login mints the state cookie carrying the fixed CSRF state.
    let (_s, _l, cookies) = get(&router, "/auth/oidc/login", None).await;
    let cookie = state_cookie_header(&cookies);

    // 2) callback echoes the same state the fake set in authorize_url.
    let (status, location, set_cookies) = get(
        &router,
        "/auth/oidc/callback?code=any-code&state=fixed-csrf-state",
        Some(&cookie),
    )
    .await;

    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(location.as_deref(), Some("/"));
    // Session cookie set, state cookie cleared.
    assert!(
        set_cookies.iter().any(|c| c.starts_with("soika_session=")),
        "session cookie set: {set_cookies:?}"
    );
    assert!(
        set_cookies
            .iter()
            .any(|c| c.starts_with("soika_oidc_state=") && c.contains("Max-Age=0")),
        "state cookie cleared: {set_cookies:?}"
    );

    // User was auto-provisioned as an OIDC account.
    let user = state
        .users
        .find_by_email(email)
        .await
        .expect("query user")
        .expect("user provisioned");
    assert_eq!(user.auth_provider, soika::domain::AuthProvider::Oidc);
    assert!(!user.is_admin);
    assert!(user.password_hash.is_empty());
}

#[tokio::test]
async fn callback_with_bad_state_redirects_with_error_and_no_session() {
    let email = "victim@example.com";
    let (router, state) = build_app(Some(Arc::new(FakeProvider::new(verified_claims(email))))).await;

    let (_s, _l, cookies) = get(&router, "/auth/oidc/login", None).await;
    let cookie = state_cookie_header(&cookies);

    // Mismatched state query param.
    let (status, location, set_cookies) = get(
        &router,
        "/auth/oidc/callback?code=any-code&state=attacker-state",
        Some(&cookie),
    )
    .await;

    assert_eq!(status, StatusCode::FOUND);
    let location = location.expect("Location header");
    assert!(location.starts_with("/login?error="), "got {location}");
    // No session cookie issued.
    assert!(
        !set_cookies.iter().any(|c| c.starts_with("soika_session=")),
        "no session cookie on failure: {set_cookies:?}"
    );
    // No user created.
    assert!(
        state.users.find_by_email(email).await.expect("query").is_none(),
        "no user provisioned on failed flow"
    );
}

#[tokio::test]
async fn callback_resolving_to_admin_is_forbidden() {
    // SSO sign-in that matches the built-in admin's email must be refused: the
    // admin keeps password login (PLAN §6.4); allowing the public IdP to assume
    // it would be an account-takeover vector. No session is issued.
    let email = "admin@example.com";
    let (router, state) = build_app(Some(Arc::new(FakeProvider::new(verified_claims(email))))).await;
    seed_user(&state, email, "supersecret", true).await;

    let (_s, _l, cookies) = get(&router, "/auth/oidc/login", None).await;
    let cookie = state_cookie_header(&cookies);

    let (status, location, set_cookies) = get(
        &router,
        "/auth/oidc/callback?code=any-code&state=fixed-csrf-state",
        Some(&cookie),
    )
    .await;

    assert_eq!(status, StatusCode::FOUND);
    assert!(
        location.as_deref().unwrap_or_default().starts_with("/login?error="),
        "admin SSO rejected to login: {location:?}"
    );
    assert!(
        !set_cookies.iter().any(|c| c.starts_with("soika_session=")),
        "no session cookie for admin SSO: {set_cookies:?}"
    );
}

#[tokio::test]
async fn callback_when_disabled_is_not_found() {
    let (router, _state) = build_app(None).await;

    let (status, _l, _c) = get(
        &router,
        "/auth/oidc/callback?code=x&state=y",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// --- Coexistence gating (PLAN §6.4, Story S5) ------------------------------

fn enabled_provider() -> Arc<dyn OidcProvider> {
    Arc::new(FakeProvider::new(verified_claims("u@example.com")))
}

#[tokio::test]
async fn login_admin_password_succeeds_when_oauth_enabled() {
    let (router, state) = build_app(Some(enabled_provider())).await;
    seed_user(&state, "admin@example.com", "supersecret", true).await;

    let (status, _json) = post_json(
        &router,
        "/auth/login",
        serde_json::json!({ "email": "admin@example.com", "password": "supersecret" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn login_non_admin_password_forbidden_when_oauth_enabled() {
    let (router, state) = build_app(Some(enabled_provider())).await;
    seed_user(&state, "user@example.com", "supersecret", false).await;

    let (status, _json) = post_json(
        &router,
        "/auth/login",
        serde_json::json!({ "email": "user@example.com", "password": "supersecret" }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn register_forbidden_when_oauth_enabled() {
    let (router, _state) = build_app(Some(enabled_provider())).await;

    let (status, _json) = post_json(
        &router,
        "/auth/register",
        serde_json::json!({
            "email": "newbie@example.com",
            "password": "supersecret",
            "display_name": "Newbie"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn login_behaviour_unchanged_when_oauth_disabled() {
    let (router, state) = build_app(None).await;
    // A plain non-admin account can still log in with its password.
    seed_user(&state, "user@example.com", "supersecret", false).await;

    let (status, _json) = post_json(
        &router,
        "/auth/login",
        serde_json::json!({ "email": "user@example.com", "password": "supersecret" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn login_wrong_password_unauthorized_when_oauth_enabled() {
    // Admin with a wrong password stays on the anti-enumeration path (401),
    // not Forbidden — Forbidden is reserved for known non-admin accounts.
    let (router, state) = build_app(Some(enabled_provider())).await;
    seed_user(&state, "admin@example.com", "supersecret", true).await;

    let (status, _json) = post_json(
        &router,
        "/auth/login",
        serde_json::json!({ "email": "admin@example.com", "password": "wrong-password" }),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
