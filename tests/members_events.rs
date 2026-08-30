//! Integration tests for team-member removal and cross-project event access.
//!
//! Drives the real application router in-process via [`tower::ServiceExt::oneshot`]
//! (no network, no containers). A fresh `:memory:` SQLite DB is seeded with two
//! teams — each owning one project — plus the users/memberships/sessions needed
//! to exercise two authorization rules end-to-end:
//!
//! - `teams::remove_member` (`src/api/teams.rs`) — the last-admin guard (Variant
//!   A: membership is team-scoped): removing the only team Admin is `409`; with
//!   two admins the first removal is `204` and the second (now the last admin)
//!   is `409`; removing a user who is not a member is a `204` no-op.
//! - `events::get` (`src/api/events.rs`) — access is scoped to the event's
//!   project: a member of project A requesting an event that lives in project B
//!   (a different team) gets `403`; a well-formed but unknown event UUID is
//!   `404`; a non-UUID id is `400`.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::auth::hash_password;
use soika::config::LockoutConfig;
use soika::domain::{AuthProvider, Id, InstanceRole, TeamRole, UserStatus};
use soika::ports::{NewProject, NewUser};
use soika::{AppState, Config, MIGRATOR, build_state, router};
use std::time::Duration;
use tower::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";

/// The DSN public key the seeded project B is reachable by (event ingestion).
const PROJECT_B_DSN: &str = "project-b-public-key";

struct Fixture {
    router: Router,
    state: AppState,
}

impl Fixture {
    async fn spawn() -> Fixture {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        MIGRATOR.run(&pool).await.expect("run migrations");
        let state = build_state(pool, test_config()).unwrap();
        Fixture {
            router: router::build(state.clone()),
            state,
        }
    }

    async fn create_user(&self, email: &str, is_admin: bool) -> Id {
        let instance_role = if is_admin {
            InstanceRole::Owner
        } else {
            InstanceRole::Member
        };
        self.state
            .users
            .create(NewUser {
                email: email.into(),
                display_name: email.into(),
                password_hash: hash_password(PASSWORD).unwrap(),
                instance_role,
                auth_provider: AuthProvider::Local,
                status: UserStatus::Active,
            })
            .await
            .unwrap()
            .id
    }

    /// Create a project under `team_id`, returning its full id (UUID) and the
    /// short public id used on the wire.
    async fn create_project(&self, team_id: Id, slug: &str, dsn: &str) -> (Id, String) {
        let project = self
            .state
            .projects
            .create(NewProject {
                team_id,
                name: format!("Project {slug}"),
                slug: slug.into(),
                dsn_public_key: dsn.into(),
                retention_events: 1000,
                retention_days: 0,
                webhook_url: None,
            })
            .await
            .unwrap();
        (project.id, project.short_id)
    }

    /// Log in via `/auth/login` and return the session cookie pair
    /// (`name=value`) to replay on authenticated requests.
    async fn login(&self, email: &str) -> String {
        let body = json!({ "email": email, "password": PASSWORD }).to_string();
        let request = Request::builder()
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

    /// Authenticated `GET` returning the status and parsed JSON body.
    async fn get(&self, uri: &str, cookie: &str) -> (StatusCode, Value) {
        self.send("GET", uri, cookie, Body::empty()).await
    }

    /// Authenticated `DELETE` returning the status and parsed JSON body.
    async fn delete(&self, uri: &str, cookie: &str) -> (StatusCode, Value) {
        self.send("DELETE", uri, cookie, Body::empty()).await
    }

    async fn send(&self, method: &str, uri: &str, cookie: &str, body: Body) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", cookie)
            .body(body)
            .unwrap();
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

    /// Ingest one event into `project_id` via the legacy DSN store endpoint and
    /// return the internal event UUID it was persisted under. Mirrors the wire
    /// path the SDKs use (no session, DSN auth only).
    async fn ingest_event(&self, project_id: Id, dsn_key: &str, event_id: &str) -> Id {
        let payload = json!({
            "event_id": event_id,
            "level": "error",
            "exception": { "values": [{ "type": "ValueError", "value": "boom" }] }
        });
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/{project_id}/store/"))
            .header(
                "x-sentry-auth",
                format!("Sentry sentry_version=7, sentry_key={dsn_key}, sentry_client=test/1.0"),
            )
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "ingest should succeed");

        // Read back the persisted internal event id via the same repository port
        // the API reads from, so the UUID matches what `events::get` resolves.
        let issues = self
            .state
            .issues
            .list(project_id, soika::ports::IssueFilter::default())
            .await
            .expect("list issues");
        assert_eq!(issues.len(), 1, "ingest creates exactly one issue");
        self.state
            .events
            .latest_for_issue(issues[0].id)
            .await
            .expect("latest event")
            .expect("an event exists")
            .id
    }
}

// --- teams::remove_member last-admin guard ----------------------------------

#[tokio::test]
async fn removing_the_only_admin_is_conflict() {
    let app = Fixture::spawn().await;
    let team = app.state.teams.create("team-a".into()).await.unwrap().id;
    let _ = app.create_project(team, "alpha", "dsn-alpha").await;

    // A single team Admin who is also the caller.
    let admin = app.create_user("admin@example.com", false).await;
    app.state
        .teams
        .add_member(team, admin, TeamRole::Admin)
        .await
        .unwrap();
    let cookie = app.login("admin@example.com").await;

    // Removing the sole admin would orphan the team → 409.
    let (status, body) = app
        .delete(&format!("/api/teams/{team}/members/{admin}"), &cookie)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"],
        "conflict: cannot remove the last admin of the team"
    );

    // The membership is untouched.
    assert_eq!(
        app.state.teams.member_role(team, admin).await.unwrap(),
        Some(TeamRole::Admin),
        "the last admin must still be a member"
    );
}

#[tokio::test]
async fn second_admin_removable_then_last_admin_is_conflict() {
    let app = Fixture::spawn().await;
    let team = app.state.teams.create("team-a".into()).await.unwrap().id;
    let _ = app.create_project(team, "alpha", "dsn-alpha").await;

    // Two team admins; the first is the caller.
    let admin_a = app.create_user("admin-a@example.com", false).await;
    let admin_b = app.create_user("admin-b@example.com", false).await;
    app.state
        .teams
        .add_member(team, admin_a, TeamRole::Admin)
        .await
        .unwrap();
    app.state
        .teams
        .add_member(team, admin_b, TeamRole::Admin)
        .await
        .unwrap();
    let cookie = app.login("admin-a@example.com").await;

    // Two admins exist, so removing the second one succeeds → 204.
    let (status, _) = app
        .delete(&format!("/api/teams/{team}/members/{admin_b}"), &cookie)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.state.teams.member_role(team, admin_b).await.unwrap(),
        None,
        "the removed admin is gone"
    );

    // admin_a is now the last admin; removing them is refused → 409.
    let (status, body) = app
        .delete(&format!("/api/teams/{team}/members/{admin_a}"), &cookie)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"],
        "conflict: cannot remove the last admin of the team"
    );
}

#[tokio::test]
async fn removing_a_non_member_is_a_noop() {
    let app = Fixture::spawn().await;
    let team = app.state.teams.create("team-a".into()).await.unwrap().id;
    let _ = app.create_project(team, "alpha", "dsn-alpha").await;

    // An admin caller (so the team-manager guard passes).
    let admin = app.create_user("admin@example.com", false).await;
    app.state
        .teams
        .add_member(team, admin, TeamRole::Admin)
        .await
        .unwrap();
    let cookie = app.login("admin@example.com").await;

    // A real, active user who simply isn't a member of this team. Removing a
    // non-member is an idempotent no-op → 204, and the admin is undisturbed.
    let stranger = app.create_user("stranger@example.com", false).await;

    let (status, _) = app
        .delete(&format!("/api/teams/{team}/members/{stranger}"), &cookie)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.state.teams.member_role(team, admin).await.unwrap(),
        Some(TeamRole::Admin),
        "the team admin is untouched"
    );
}

// --- events::get cross-project authorization -------------------------------

#[tokio::test]
async fn member_of_project_a_cannot_read_event_in_project_b() {
    let app = Fixture::spawn().await;

    // Two teams, one project each.
    let team_a = app.state.teams.create("team-a".into()).await.unwrap().id;
    let team_b = app.state.teams.create("team-b".into()).await.unwrap().id;
    let (_project_a_id, _project_a_short) = app.create_project(team_a, "alpha", "dsn-alpha").await;
    let (project_b_id, _project_b_short) = app.create_project(team_b, "beta", PROJECT_B_DSN).await;

    // A user who belongs only to team-a (no access to team-b's project).
    let member_a = app.create_user("a@example.com", false).await;
    app.state
        .teams
        .add_member(team_a, member_a, TeamRole::Contributor)
        .await
        .unwrap();
    let cookie = app.login("a@example.com").await;

    // An event that lives in project B.
    let event_id = app
        .ingest_event(
            project_b_id,
            PROJECT_B_DSN,
            "aaaaaaaaaaaa4aaaaaaaaaaaaaaaaaaa",
        )
        .await;

    let (status, body) = app.get(&format!("/api/events/{event_id}"), &cookie).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "foreign-project event is 403"
    );
    assert_eq!(body["error"], "you do not have access to this project");
}

#[tokio::test]
async fn unknown_event_uuid_is_not_found() {
    let app = Fixture::spawn().await;
    let team = app.state.teams.create("team-a".into()).await.unwrap().id;
    let _ = app.create_project(team, "alpha", "dsn-alpha").await;

    let member = app.create_user("a@example.com", false).await;
    app.state
        .teams
        .add_member(team, member, TeamRole::Contributor)
        .await
        .unwrap();
    let cookie = app.login("a@example.com").await;

    // A well-formed UUID that matches no stored event.
    let missing = Id::new_v4();
    let (status, body) = app.get(&format!("/api/events/{missing}"), &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "event not found");
}

#[tokio::test]
async fn non_uuid_event_id_is_bad_request() {
    let app = Fixture::spawn().await;
    let team = app.state.teams.create("team-a".into()).await.unwrap().id;
    let _ = app.create_project(team, "alpha", "dsn-alpha").await;

    let member = app.create_user("a@example.com", false).await;
    app.state
        .teams
        .add_member(team, member, TeamRole::Contributor)
        .await
        .unwrap();
    let cookie = app.login("a@example.com").await;

    // A path id that is not a UUID is rejected before any lookup → 400.
    let (status, body) = app.get("/api/events/not-a-uuid", &cookie).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid id");
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
