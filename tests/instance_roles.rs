//! Integration tests for the instance-role gates (`Owner | Manager | Member`).
//!
//! Drives the *real* application router in-process via
//! [`tower::ServiceExt::oneshot`] (no network, no containers) against a fresh
//! `:memory:` SQLite DB seeded through the repository ports. These pin the new
//! RBAC instance-level authorization introduced in the Variant-A model:
//!
//! - `require_manager` (`Owner | Manager`) gates instance-management routes:
//!   creating/deleting teams, listing all teams, changing a user's instance role.
//!   A plain `Member` is `403`; a `Manager` and an `Owner` succeed.
//! - `require_owner` (`Owner` only) gates granting/revoking the `Owner` instance
//!   role: a `Manager` cannot grant or revoke `Owner` (`403`); an `Owner` can.
//! - **Last-Owner protection:** demoting the only remaining `Owner` is refused
//!   (`409`), so the instance can never be left without an `Owner`.
//! - **Closed membership:** `GET /teams` shows a `Member` only the teams they
//!   belong to, while `Owner | Manager` see every team.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::auth::hash_password;
use soika::config::LockoutConfig;
use soika::domain::{AuthProvider, Id, InstanceRole, TeamRole, UserStatus};
use soika::ports::NewUser;
use soika::{AppState, Config, MIGRATOR, build_state, router};
use std::time::Duration;
use tower::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";

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
        let state = build_state(pool, test_config());
        Fixture {
            router: router::build(state.clone()),
            state,
        }
    }

    /// Seed an active local user with the given instance role; returns its id.
    async fn create_user(&self, email: &str, role: InstanceRole) -> Id {
        self.state
            .users
            .create(NewUser {
                email: email.into(),
                display_name: email.into(),
                password_hash: hash_password(PASSWORD).unwrap(),
                instance_role: role,
                auth_provider: AuthProvider::Local,
                status: UserStatus::Active,
            })
            .await
            .unwrap()
            .id
    }

    /// Log in via `/auth/login` and return the session cookie pair to replay.
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

    async fn get(&self, uri: &str, cookie: &str) -> (StatusCode, Value) {
        self.send("GET", uri, cookie, Body::empty()).await
    }

    async fn delete(&self, uri: &str, cookie: &str) -> (StatusCode, Value) {
        self.send("DELETE", uri, cookie, Body::empty()).await
    }

    async fn post_json(&self, uri: &str, cookie: &str, body: &Value) -> (StatusCode, Value) {
        self.send_json("POST", uri, cookie, body).await
    }

    async fn patch_json(&self, uri: &str, cookie: &str, body: &Value) -> (StatusCode, Value) {
        self.send_json("PATCH", uri, cookie, body).await
    }

    async fn send_json(
        &self,
        method: &str,
        uri: &str,
        cookie: &str,
        body: &Value,
    ) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.run(request).await
    }

    async fn send(&self, method: &str, uri: &str, cookie: &str, body: Body) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", cookie)
            .body(body)
            .unwrap();
        self.run(request).await
    }

    async fn run(&self, request: Request<Body>) -> (StatusCode, Value) {
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
}

// ---------------------------------------------------------------------------
// require_manager: create/delete team is Owner|Manager only.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn member_cannot_create_team_but_manager_and_owner_can() {
    let app = Fixture::spawn().await;
    app.create_user("member@example.com", InstanceRole::Member)
        .await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    app.create_user("owner@example.com", InstanceRole::Owner)
        .await;

    // A plain Member cannot create a team (require_manager) → 403.
    let member_cookie = app.login("member@example.com").await;
    let (status, _) = app
        .post_json(
            "/api/teams",
            &member_cookie,
            &json!({ "name": "Members Team" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a Member cannot create a team"
    );

    // A Manager can.
    let manager_cookie = app.login("manager@example.com").await;
    let (status, _) = app
        .post_json("/api/teams", &manager_cookie, &json!({ "name": "Ops" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "a Manager may create a team");

    // An Owner can.
    let owner_cookie = app.login("owner@example.com").await;
    let (status, _) = app
        .post_json("/api/teams", &owner_cookie, &json!({ "name": "Platform" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "an Owner may create a team");
}

#[tokio::test]
async fn member_cannot_delete_team_but_manager_can() {
    let app = Fixture::spawn().await;
    app.create_user("member@example.com", InstanceRole::Member)
        .await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    let team = app.state.teams.create("Ops".into()).await.unwrap().id;

    // A Member cannot delete a team → 403, and the team survives.
    let member_cookie = app.login("member@example.com").await;
    let (status, _) = app
        .delete(&format!("/api/teams/{team}"), &member_cookie)
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a Member cannot delete a team"
    );
    assert!(
        app.state.teams.find_by_id(team).await.unwrap().is_some(),
        "the team is untouched"
    );

    // A Manager can delete it → 204.
    let manager_cookie = app.login("manager@example.com").await;
    let (status, _) = app
        .delete(&format!("/api/teams/{team}"), &manager_cookie)
        .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "a Manager may delete a team"
    );
    assert!(
        app.state.teams.find_by_id(team).await.unwrap().is_none(),
        "the team is gone"
    );
}

// ---------------------------------------------------------------------------
// require_manager: changing a user's instance role is Owner|Manager only.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn member_cannot_change_instance_roles() {
    let app = Fixture::spawn().await;
    app.create_user("member@example.com", InstanceRole::Member)
        .await;
    let target = app
        .create_user("target@example.com", InstanceRole::Member)
        .await;

    let member_cookie = app.login("member@example.com").await;
    let (status, _) = app
        .patch_json(
            &format!("/api/admin/users/{target}"),
            &member_cookie,
            &json!({ "instance_role": "manager" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a Member cannot manage instance roles"
    );
    // The target's role is unchanged.
    let still = app.state.users.find_by_id(target).await.unwrap().unwrap();
    assert_eq!(still.instance_role, InstanceRole::Member);
}

#[tokio::test]
async fn manager_can_promote_member_to_manager() {
    let app = Fixture::spawn().await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    let target = app
        .create_user("target@example.com", InstanceRole::Member)
        .await;

    let manager_cookie = app.login("manager@example.com").await;
    let (status, body) = app
        .patch_json(
            &format!("/api/admin/users/{target}"),
            &manager_cookie,
            &json!({ "instance_role": "manager" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a Manager may grant Manager/Member roles"
    );
    assert_eq!(body["instance_role"], "manager");
    let updated = app.state.users.find_by_id(target).await.unwrap().unwrap();
    assert_eq!(updated.instance_role, InstanceRole::Manager);
}

// ---------------------------------------------------------------------------
// require_owner: granting / revoking the Owner role is Owner-only.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn manager_cannot_grant_owner_role() {
    let app = Fixture::spawn().await;
    app.create_user("owner@example.com", InstanceRole::Owner)
        .await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    let target = app
        .create_user("target@example.com", InstanceRole::Member)
        .await;

    let manager_cookie = app.login("manager@example.com").await;
    let (status, _) = app
        .patch_json(
            &format!("/api/admin/users/{target}"),
            &manager_cookie,
            &json!({ "instance_role": "owner" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only an Owner may grant the Owner role"
    );
    let still = app.state.users.find_by_id(target).await.unwrap().unwrap();
    assert_eq!(still.instance_role, InstanceRole::Member);
}

#[tokio::test]
async fn manager_cannot_revoke_owner_role() {
    let app = Fixture::spawn().await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    // Two Owners so last-Owner protection would not interfere; the gate must be
    // the Owner-only rule, not the count.
    app.create_user("owner-a@example.com", InstanceRole::Owner)
        .await;
    let owner_b = app
        .create_user("owner-b@example.com", InstanceRole::Owner)
        .await;

    let manager_cookie = app.login("manager@example.com").await;
    let (status, _) = app
        .patch_json(
            &format!("/api/admin/users/{owner_b}"),
            &manager_cookie,
            &json!({ "instance_role": "member" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only an Owner may revoke the Owner role"
    );
    let still = app.state.users.find_by_id(owner_b).await.unwrap().unwrap();
    assert_eq!(still.instance_role, InstanceRole::Owner);
}

#[tokio::test]
async fn owner_can_grant_and_revoke_owner_role() {
    let app = Fixture::spawn().await;
    app.create_user("owner@example.com", InstanceRole::Owner)
        .await;
    let target = app
        .create_user("target@example.com", InstanceRole::Member)
        .await;

    let owner_cookie = app.login("owner@example.com").await;

    // Grant Owner.
    let (status, body) = app
        .patch_json(
            &format!("/api/admin/users/{target}"),
            &owner_cookie,
            &json!({ "instance_role": "owner" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "an Owner may grant Owner");
    assert_eq!(body["instance_role"], "owner");
    assert_eq!(app.state.users.count_owners().await.unwrap(), 2);

    // Revoke it back to Member (two Owners exist, so last-Owner protection
    // does not apply).
    let (status, body) = app
        .patch_json(
            &format!("/api/admin/users/{target}"),
            &owner_cookie,
            &json!({ "instance_role": "member" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "an Owner may revoke Owner");
    assert_eq!(body["instance_role"], "member");
    assert_eq!(app.state.users.count_owners().await.unwrap(), 1);
}

// ---------------------------------------------------------------------------
// Last-Owner protection: the only Owner cannot be demoted.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cannot_demote_the_last_owner() {
    let app = Fixture::spawn().await;
    let owner = app
        .create_user("owner@example.com", InstanceRole::Owner)
        .await;

    let owner_cookie = app.login("owner@example.com").await;
    let (status, body) = app
        .patch_json(
            &format!("/api/admin/users/{owner}"),
            &owner_cookie,
            &json!({ "instance_role": "member" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the last Owner cannot be demoted"
    );
    assert_eq!(body["error"], "conflict: cannot demote the last owner");

    // The Owner is untouched.
    let still = app.state.users.find_by_id(owner).await.unwrap().unwrap();
    assert_eq!(still.instance_role, InstanceRole::Owner);
    assert_eq!(app.state.users.count_owners().await.unwrap(), 1);
}

// ---------------------------------------------------------------------------
// Closed membership: GET /teams scope by instance role.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn member_sees_only_own_teams_while_manager_sees_all() {
    let app = Fixture::spawn().await;
    let member = app
        .create_user("member@example.com", InstanceRole::Member)
        .await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;

    let team_a = app.state.teams.create("team-a".into()).await.unwrap().id;
    let team_b = app.state.teams.create("team-b".into()).await.unwrap().id;
    // The member belongs only to team-a.
    app.state
        .teams
        .add_member(team_a, member, TeamRole::Contributor)
        .await
        .unwrap();

    // The Member sees exactly one team (their own).
    let member_cookie = app.login("member@example.com").await;
    let (status, body) = app.get("/api/teams", &member_cookie).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<String> = body
        .as_array()
        .expect("team list")
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec![team_a.to_string()], "Member sees only their team");

    // The Manager sees both teams.
    let manager_cookie = app.login("manager@example.com").await;
    let (status, body) = app.get("/api/teams", &manager_cookie).await;
    assert_eq!(status, StatusCode::OK);
    let mut ids: Vec<String> = body
        .as_array()
        .expect("team list")
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    let mut expected = vec![team_a.to_string(), team_b.to_string()];
    expected.sort();
    assert_eq!(ids, expected, "Manager sees every team");
}

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
            window: Duration::from_secs(900),
            base_lockout: Duration::from_secs(60),
            max_lockout: Duration::from_secs(3600),
        },
        timezone: "UTC".to_string(),
    }
}
