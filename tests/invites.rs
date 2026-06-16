//! Integration tests for the invite-acceptance flow (`POST /api/invite/{token}`).
//!
//! These drive the *real* application router in-process via
//! [`tower::ServiceExt::oneshot`] — no network, no containers. Each test builds
//! a fresh `:memory:` SQLite database, runs the migrations, seeds a team +
//! project, then mints invites the same way the codebase does (a `NewInvite`
//! row inserted through the [`InviteRepository`] port, keyed by a CSPRNG token
//! from [`generate_invite_token`]). Assertions go through the same repository
//! ports the handler uses, so the tests prove the accept pipeline end-to-end:
//! token lookup → usability check → resolve/register invitee → membership
//! upsert → mark accepted.
//!
//! The branches covered mirror `src/auth/handlers.rs::accept_invite` /
//! `resolve_or_register_invitee`:
//!   * a new email registers then joins at the invited role,
//!   * an expired invite is `401`,
//!   * an already-accepted invite is rejected (`401`),
//!   * an existing account with the wrong password is `401`,
//!   * under OIDC-only, a password accept for a *new* email is `403`.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chrono::Duration;
use openidconnect::url::Url;
use openidconnect::{CsrfToken, Nonce, PkceCodeChallenge, PkceCodeVerifier};
use serde_json::{Value, json};
use soika::auth::{
    AuthorizeRequest, OidcClaims, OidcProvider, generate_invite_token, hash_password,
};
use soika::config::LockoutConfig;
use soika::domain::{AuthProvider, Id, Role, UserStatus};
use soika::ports::{NewInvite, NewProject, NewUser};
use soika::{AppState, Config, MIGRATOR, build_state, router};
use tower::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";

/// A running application under test: the wired router plus the state (so tests
/// can mint invites and assert persisted rows via the ports).
struct Fixture {
    router: Router,
    state: AppState,
}

impl Fixture {
    /// Build a fresh in-memory app with SSO disabled (password flows enabled).
    async fn spawn() -> Fixture {
        Self::spawn_with_oidc(None).await
    }

    /// Like [`spawn`], but lets a test enable SSO by passing a provider, which
    /// flips `accept_invite` onto its OIDC-only branch.
    async fn spawn_with_oidc(oidc: Option<Arc<dyn OidcProvider>>) -> Fixture {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        MIGRATOR.run(&pool).await.expect("run migrations");
        let state = build_state(pool, test_config()).with_oidc(oidc);
        Fixture {
            router: router::build(state.clone()),
            state,
        }
    }

    /// Seed a team with one project and return the project's internal id.
    async fn seed_project(&self) -> Id {
        let team = self
            .state
            .teams
            .create("test-team".into())
            .await
            .expect("create team");
        self.state
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
            .expect("create project")
            .id
    }

    /// Create an active local user with the shared test password.
    async fn create_user(&self, email: &str) -> Id {
        self.state
            .users
            .create(NewUser {
                email: email.into(),
                display_name: email.into(),
                password_hash: hash_password(PASSWORD).unwrap(),
                is_admin: false,
                auth_provider: AuthProvider::Local,
                status: UserStatus::Active,
            })
            .await
            .unwrap()
            .id
    }

    /// Mint an invite exactly as the codebase does: a CSPRNG token persisted as
    /// a `NewInvite` row. `expires_in` is relative to *now*, so a negative value
    /// produces an already-expired invite. Returns the token.
    async fn mint_invite(
        &self,
        project_id: Id,
        role: Role,
        email: Option<&str>,
        expires_in: Duration,
    ) -> String {
        let token = generate_invite_token();
        self.state
            .invites
            .create(NewInvite {
                token: token.clone(),
                project_id,
                role,
                email: email.map(str::to_owned),
                created_by: None,
                expires_at: self.state.clock.now() + expires_in,
            })
            .await
            .expect("create invite");
        token
    }

    /// `POST /api/invite/{token}` with an optional JSON body; returns the status
    /// and the parsed JSON response (or `Value::Null` for an empty body).
    ///
    /// The JSON accept endpoint is namespaced under `/api`; the bare
    /// `/invite/{token}` path is the SPA client route served by the fallback (and
    /// would always answer `200` with the app shell), so the `/api` prefix is
    /// load-bearing here.
    async fn accept(&self, token: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/api/invite/{token}"));
        let body = match body {
            Some(v) => {
                builder = builder.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let request = builder.body(body).expect("build request");
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, json)
    }

    /// The persisted membership of `user_id` in `project_id`, if any.
    async fn membership_role(&self, project_id: Id, user_id: Id) -> Option<Role> {
        self.state
            .memberships
            .find(project_id, user_id)
            .await
            .expect("find membership")
            .map(|m| m.role)
    }
}

/// A minimal config: in-memory db, no SMTP, signup disabled, lockout off so the
/// brute-force guard never interferes with the wrong-password assertions.
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
        lockout: LockoutConfig {
            enabled: false,
            max_attempts: 100,
            window: StdDuration::from_secs(900),
            base_lockout: StdDuration::from_secs(60),
            max_lockout: StdDuration::from_secs(3600),
        },
        timezone: "UTC".to_string(),
    }
}

// --- OIDC fake (mirrors tests/oidc.rs) -------------------------------------

/// A scripted [`OidcProvider`] fake. Only its presence matters for the invite
/// tests — enabling SSO flips `resolve_or_register_invitee` onto the branch
/// that refuses to mint a password-backed account from an invite.
struct FakeProvider;

#[async_trait]
impl OidcProvider for FakeProvider {
    fn authorize_url(&self) -> AuthorizeRequest {
        let (_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        AuthorizeRequest {
            url: Url::parse("https://idp.example.com/oauth/authorize?response_type=code").unwrap(),
            csrf_state: CsrfToken::new("fixed-csrf-state".into()),
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
        Ok(OidcClaims {
            email: "u@example.com".into(),
            email_verified: true,
            display_name: "Test User".into(),
        })
    }

    fn provider_name(&self) -> &str {
        "GitLab"
    }

    fn allowed_email_domains(&self) -> &[String] {
        &[]
    }

    fn require_approval(&self) -> bool {
        false
    }
}

// --- New-email registration branch -----------------------------------------

#[tokio::test]
async fn new_email_accepts_invite_and_becomes_member_at_invited_role() {
    let app = Fixture::spawn().await;
    let project_id = app.seed_project().await;
    let token = app
        .mint_invite(
            project_id,
            Role::Member,
            Some("invitee@example.com"),
            Duration::days(7),
        )
        .await;

    // A brand-new email supplies a password + display name and joins.
    let (status, body) = app
        .accept(
            &token,
            Some(json!({
                "email": "invitee@example.com",
                "password": PASSWORD,
                "display_name": "Invitee",
            })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], "invitee@example.com");
    // Password hash is never echoed to clients.
    assert!(body.get("password_hash").is_none());

    // The account was provisioned as a local user.
    let user = app
        .state
        .users
        .find_by_email("invitee@example.com")
        .await
        .expect("query user")
        .expect("user provisioned from invite");
    assert_eq!(user.auth_provider, AuthProvider::Local);
    assert!(!user.is_admin);

    // Membership row created at the invited role.
    assert_eq!(
        app.membership_role(project_id, user.id).await,
        Some(Role::Member),
        "invitee joins the project as a Member"
    );

    // The invite is now marked accepted (single-use).
    let invite = app
        .state
        .invites
        .find_by_token(&token)
        .await
        .expect("query invite")
        .expect("invite exists");
    assert!(invite.accepted_at.is_some(), "invite consumed on accept");
}

#[tokio::test]
async fn new_email_accepts_invite_at_admin_role() {
    // The invited role is authoritative: an Admin invite yields an Admin
    // membership, not the default Member.
    let app = Fixture::spawn().await;
    let project_id = app.seed_project().await;
    let token = app
        .mint_invite(
            project_id,
            Role::Admin,
            Some("boss@example.com"),
            Duration::days(7),
        )
        .await;

    let (status, _body) = app
        .accept(
            &token,
            Some(json!({
                "email": "boss@example.com",
                "password": PASSWORD,
                "display_name": "Boss",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let user = app
        .state
        .users
        .find_by_email("boss@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        app.membership_role(project_id, user.id).await,
        Some(Role::Admin),
    );
}

// --- Expired invite --------------------------------------------------------

#[tokio::test]
async fn expired_invite_is_unauthorized() {
    let app = Fixture::spawn().await;
    let project_id = app.seed_project().await;
    // expires_at one second in the past → unusable at the handler's clock.now().
    let token = app
        .mint_invite(
            project_id,
            Role::Member,
            Some("late@example.com"),
            Duration::seconds(-1),
        )
        .await;

    let (status, _body) = app
        .accept(
            &token,
            Some(json!({
                "email": "late@example.com",
                "password": PASSWORD,
                "display_name": "Late",
            })),
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // No account created and no membership for an expired invite.
    assert!(
        app.state
            .users
            .find_by_email("late@example.com")
            .await
            .unwrap()
            .is_none(),
        "expired invite provisions nothing"
    );
}

// --- Already-accepted invite -----------------------------------------------

#[tokio::test]
async fn already_accepted_invite_is_rejected() {
    let app = Fixture::spawn().await;
    let project_id = app.seed_project().await;
    let token = app
        .mint_invite(
            project_id,
            Role::Member,
            Some("twice@example.com"),
            Duration::days(7),
        )
        .await;

    // Mark it accepted out-of-band (as a prior successful accept would have).
    app.state
        .invites
        .mark_accepted(&token, app.state.clock.now())
        .await
        .expect("mark accepted");

    let (status, _body) = app
        .accept(
            &token,
            Some(json!({
                "email": "twice@example.com",
                "password": PASSWORD,
                "display_name": "Twice",
            })),
        )
        .await;

    // An already-consumed invite is unusable → 401, no new account.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        app.state
            .users
            .find_by_email("twice@example.com")
            .await
            .unwrap()
            .is_none(),
        "a consumed invite cannot provision a second account"
    );
}

// --- Existing account, wrong password --------------------------------------

#[tokio::test]
async fn existing_account_with_wrong_password_is_unauthorized() {
    let app = Fixture::spawn().await;
    let project_id = app.seed_project().await;
    let user_id = app.create_user("member@example.com").await;
    let token = app
        .mint_invite(
            project_id,
            Role::Member,
            Some("member@example.com"),
            Duration::days(7),
        )
        .await;

    // The email matches an existing account, but the password is wrong: the
    // existing-account branch must authenticate before joining.
    let (status, _body) = app
        .accept(
            &token,
            Some(json!({
                "email": "member@example.com",
                "password": "not-the-right-password",
            })),
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // No membership created on a failed credential check.
    assert_eq!(
        app.membership_role(project_id, user_id).await,
        None,
        "a wrong password does not grant membership"
    );
    // And the invite is still usable (not consumed by a failed attempt).
    let invite = app
        .state
        .invites
        .find_by_token(&token)
        .await
        .unwrap()
        .unwrap();
    assert!(
        invite.accepted_at.is_none(),
        "failed accept leaves invite open"
    );
}

#[tokio::test]
async fn existing_account_with_correct_password_joins() {
    // The positive counterpart: the right password authenticates the existing
    // account and joins it at the invited role.
    let app = Fixture::spawn().await;
    let project_id = app.seed_project().await;
    let user_id = app.create_user("member@example.com").await;
    let token = app
        .mint_invite(
            project_id,
            Role::Member,
            Some("member@example.com"),
            Duration::days(7),
        )
        .await;

    let (status, body) = app
        .accept(
            &token,
            Some(json!({
                "email": "member@example.com",
                "password": PASSWORD,
            })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], "member@example.com");
    assert_eq!(
        app.membership_role(project_id, user_id).await,
        Some(Role::Member),
    );
}

// --- OIDC-only branch ------------------------------------------------------

#[tokio::test]
async fn password_accept_for_new_email_is_forbidden_when_oidc_only() {
    // With SSO enabled, `resolve_or_register_invitee` refuses to mint a
    // password-backed account from an invite for an unknown email — the invitee
    // must sign in via SSO first. Surfaced as 403 Forbidden.
    let app = Fixture::spawn_with_oidc(Some(Arc::new(FakeProvider))).await;
    let project_id = app.seed_project().await;
    let token = app
        .mint_invite(
            project_id,
            Role::Member,
            Some("sso-only@example.com"),
            Duration::days(7),
        )
        .await;

    let (status, _body) = app
        .accept(
            &token,
            Some(json!({
                "email": "sso-only@example.com",
                "password": PASSWORD,
                "display_name": "SSO Only",
            })),
        )
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    // No password account minted under SSO-only invite acceptance.
    assert!(
        app.state
            .users
            .find_by_email("sso-only@example.com")
            .await
            .unwrap()
            .is_none(),
        "OIDC-only invite accept does not provision a password account"
    );
}
