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

mod common;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::auth::hash_password;
use soika::config::LockoutConfig;
use soika::domain::{AuthProvider, Id, InstanceRole, TeamRole, UserStatus};
use soika::ports::NewUser;
use soika::{AppState, Config, build_state, router};
use std::time::Duration;
use tower::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";

struct Fixture {
    router: Router,
    state: AppState,
}

impl Fixture {
    /// Simulate a restart with new config while retaining users and sessions.
    fn set_admin_users(&mut self, emails: &str) {
        std::sync::Arc::make_mut(&mut self.state.config).admin_users =
            soika::config::AdminUsers::parse(emails).unwrap();
        self.router = router::build(self.state.clone());
    }

    async fn spawn() -> Fixture {
        let pool = common::test_db().await;
        let state = build_state(pool, test_config()).unwrap();
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
            .header("x-soika-request", "1")
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
            .header("x-soika-request", "1")
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

#[tokio::test]
async fn admin_users_registration_rejects_reserved_emails() {
    let mut app = Fixture::spawn().await;
    app.set_admin_users(" ALICE@EXAMPLE.COM, bob@example.com ");
    app.state.settings.set_allow_signup(true).await.unwrap();
    for email in ["alice@example.com", "BOB@example.com"] {
        let (status, _) = app
            .post_json(
                "/auth/register",
                "",
                &json!({"email":email,"password":PASSWORD,"display_name":"Visitor"}),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            app.state
                .users
                .find_by_email(&email.to_ascii_lowercase())
                .await
                .unwrap()
                .is_none()
        );
    }
    let (status, user) = app
        .post_json(
            "/auth/register",
            "",
            &json!({"email":"other@example.com","password":PASSWORD,"display_name":"Visitor"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(user["instance_role"], "member");
}

#[tokio::test]
async fn admin_users_password_login_ignores_email_case() {
    let mut app = Fixture::spawn().await;
    app.set_admin_users("alice@example.com");
    app.create_user("alice@example.com", InstanceRole::Member)
        .await;
    let (status, user) = app
        .post_json(
            "/auth/login",
            "",
            &json!({"email": "ALICE@EXAMPLE.COM", "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["instance_role"], "owner");
    let cookie = app.login("aLiCe@eXaMpLe.cOm").await;
    assert_eq!(app.get("/api/settings", &cookie).await.0, StatusCode::OK);
    assert_eq!(
        app.get("/auth/config", &cookie).await.1["user"]["instance_role"],
        "owner"
    );
    assert_eq!(
        app.state
            .users
            .find_by_email("alice@example.com")
            .await
            .unwrap()
            .unwrap()
            .instance_role,
        InstanceRole::Member
    );
}

#[tokio::test]
async fn admin_users_grants_owner_operations_and_profile_without_team_membership() {
    let mut app = Fixture::spawn().await;
    app.set_admin_users("admin@example.com");
    app.create_user("admin@example.com", InstanceRole::Member)
        .await;
    let other = app
        .create_user("other@example.com", InstanceRole::Member)
        .await;
    let cookie = app.login("admin@example.com").await;
    let (status, config) = app.get("/auth/config", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["user"]["instance_role"], "owner");
    let (status, profile) = app
        .patch_json("/api/profile", &cookie, &json!({"display_name": "Renamed"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(profile["instance_role"], "owner");
    let (status, team) = app
        .post_json("/api/teams", &cookie, &json!({"name": "Private"}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app
        .get(
            &format!("/api/teams/{}", team["id"].as_str().unwrap()),
            &cookie,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, project) = app
        .post_json(
            "/api/projects",
            &cookie,
            &json!({"name": "Private Project", "team_id": team["id"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let project_path = format!("/api/projects/{}", project["id"].as_str().unwrap());
    assert_eq!(app.get(&project_path, &cookie).await.0, StatusCode::OK);
    app.set_admin_users("");
    assert_eq!(
        app.get(&project_path, &cookie).await.0,
        StatusCode::FORBIDDEN
    );
    app.set_admin_users("admin@example.com");
    let (status, _) = app
        .patch_json(
            &format!("/api/admin/users/{other}"),
            &cookie,
            &json!({"instance_role": "owner"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "environment admin can grant Owner");
    let (status, _) = app
        .patch_json(
            &format!("/api/admin/users/{other}"),
            &cookie,
            &json!({"instance_role": "member"}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "keep the last stored Owner for recovery"
    );
}

#[tokio::test]
async fn admin_users_changes_affect_existing_sessions_and_preserve_ui_roles() {
    let mut app = Fixture::spawn().await;
    for stored in [
        InstanceRole::Member,
        InstanceRole::Manager,
        InstanceRole::Owner,
    ] {
        let email = format!("{}@example.com", stored.as_str());
        app.create_user(&email, stored).await;
        let cookie = app.login(&email).await;
        app.set_admin_users(&email);
        let (_, config) = app.get("/auth/config", &cookie).await;
        assert_eq!(config["user"]["instance_role"], "owner");
        assert_eq!(app.get("/api/settings", &cookie).await.0, StatusCode::OK);
        app.set_admin_users("");
        let (_, config) = app.get("/auth/config", &cookie).await;
        assert_eq!(config["user"]["instance_role"], stored.as_str());
        assert_eq!(
            app.get("/api/settings", &cookie).await.0,
            if stored == InstanceRole::Member {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::OK
            }
        );
        assert_eq!(
            app.state
                .users
                .find_by_email(&email)
                .await
                .unwrap()
                .unwrap()
                .instance_role,
            stored
        );
    }
}

#[tokio::test]
async fn admin_users_shows_locked_owner_and_rejects_role_changes() {
    use soika::domain::config_file::ManagementSource;
    for source in [ManagementSource::Ui, ManagementSource::Yaml] {
        let mut app = Fixture::spawn().await;
        let config = std::sync::Arc::make_mut(&mut app.state.config);
        config.management.team_source = source;
        config.management.project_source = source;
        for role in [
            InstanceRole::Member,
            InstanceRole::Manager,
            InstanceRole::Owner,
        ] {
            let email = format!("{}@example.com", role.as_str());
            let id = app.create_user(&email, role).await;
            app.set_admin_users(&email.to_uppercase());
            let cookie = app.login(&email).await;
            let (status, users) = app.get("/api/admin/users", &cookie).await;
            assert_eq!(status, StatusCode::OK);
            let row = users
                .as_array()
                .unwrap()
                .iter()
                .find(|u| u["id"] == id.to_string())
                .unwrap();
            assert_eq!(row["instance_role"], "owner");
            assert_eq!(row["role_locked"], true);
            assert_eq!(row["stored_instance_role"], role.as_str());
            for new_role in ["member", "manager", "owner"] {
                assert_eq!(
                    app.patch_json(
                        &format!("/api/admin/users/{id}"),
                        &cookie,
                        &json!({"instance_role": new_role})
                    )
                    .await
                    .0,
                    StatusCode::FORBIDDEN
                );
            }
            app.set_admin_users("");
            let (_, users) = app.get("/api/admin/users", &cookie).await;
            if role != InstanceRole::Member {
                let row = users
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|u| u["id"] == id.to_string())
                    .unwrap();
                assert_eq!(row["instance_role"], role.as_str());
                assert_eq!(row["role_locked"], false);
            }
            assert_eq!(
                app.state
                    .users
                    .find_by_id(id)
                    .await
                    .unwrap()
                    .unwrap()
                    .instance_role,
                role
            );
        }
    }
}

#[tokio::test]
async fn admin_users_manager_cannot_delete_or_edit_environment_owner() {
    let mut app = Fixture::spawn().await;
    let target = app
        .create_user("admin@example.com", InstanceRole::Member)
        .await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    app.set_admin_users("admin@example.com");
    let cookie = app.login("manager@example.com").await;
    assert_eq!(
        app.delete(&format!("/api/admin/users/{target}"), &cookie)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.patch_json(
            &format!("/api/admin/users/{target}"),
            &cookie,
            &json!({"instance_role": "manager"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn admin_users_does_not_bypass_credentials_or_signup_gate() {
    let mut app = Fixture::spawn().await;
    app.create_user("admin@example.com", InstanceRole::Member)
        .await;
    app.set_admin_users("admin@example.com");
    assert_eq!(
        app.post_json(
            "/auth/login",
            "",
            &json!({"email":"admin@example.com", "password":"wrong-password"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.post_json(
            "/auth/register",
            "",
            &json!({"email":"new@example.com", "password":PASSWORD, "display_name":"New"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get("/api/settings", "").await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn default_team_settings_require_instance_manager_and_validate_ids() {
    let app = Fixture::spawn().await;
    for (email, role) in [
        ("member@example.com", InstanceRole::Member),
        ("manager@example.com", InstanceRole::Manager),
        ("owner@example.com", InstanceRole::Owner),
    ] {
        app.create_user(email, role).await;
    }
    let team = app.state.teams.create("Default".into()).await.unwrap().id;
    let (status, _) = app
        .patch_json("/api/settings", "", &json!({"default_team_ids": [team]}))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let member_cookie = app.login("member@example.com").await;
    let (status, _) = app
        .patch_json(
            "/api/settings",
            &member_cookie,
            &json!({"default_team_ids": [team]}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for email in ["manager@example.com", "owner@example.com"] {
        let cookie = app.login(email).await;
        let (status, view) = app.get("/api/settings", &cookie).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["default_team_ids"], json!([]));
        let (status, view) = app
            .patch_json(
                "/api/settings",
                &cookie,
                &json!({"default_team_ids": [team, team]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["default_team_ids"], json!([team]));
        let (status, _) = app.patch_json("/api/settings", &cookie, &json!({"default_team_ids": [Id::new_v4()], "org_name": "Must not change", "allow_signup": true})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = app
            .patch_json(
                "/api/settings",
                &cookie,
                &json!({"default_team_ids": [], "org_name": "  "}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = app
            .patch_json(
                "/api/settings",
                &cookie,
                &json!({"default_team_ids": ["invalid-id"]}),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, view) = app.get("/api/settings", &cookie).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["default_team_ids"], json!([team]));
        assert_eq!(view["org_name"], "soika");
        assert_eq!(view["allow_signup"], false);
        // Updating unrelated fields must preserve defaults.
        app.patch_json("/api/settings", &cookie, &json!({"org_name": "soika"}))
            .await;
        assert_eq!(
            app.state.settings.default_team_ids().await.unwrap(),
            vec![team]
        );
        let (status, view) = app
            .patch_json("/api/settings", &cookie, &json!({"default_team_ids": []}))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["default_team_ids"], json!([]));
    }
}

#[tokio::test]
async fn registration_uses_defaults_but_existing_login_and_approval_do_not() {
    let app = Fixture::spawn().await;
    app.state.settings.set_allow_signup(true).await.unwrap();
    let team = app.state.teams.create("Default".into()).await.unwrap().id;
    let (status, old) = app
        .post_json(
            "/auth/register",
            "",
            &json!({"email": "old@example.com", "password": PASSWORD, "display_name": "Old"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let old = Id::parse_str(old["id"].as_str().unwrap()).unwrap();
    assert!(app.state.teams.list_for_user(old).await.unwrap().is_empty());
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    let cookie = app.login("manager@example.com").await;
    let (status, _) = app
        .patch_json(
            "/api/settings",
            &cookie,
            &json!({"default_team_ids": [team]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    app.login("old@example.com").await;
    app.post_json(
        &format!("/api/admin/users/{old}/approve"),
        &cookie,
        &json!({}),
    )
    .await;
    assert!(app.state.teams.list_for_user(old).await.unwrap().is_empty());
    let (status, new) = app
        .post_json(
            "/auth/register",
            "",
            &json!({"email": "new@example.com", "password": PASSWORD, "display_name": "New"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let new = Id::parse_str(new["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        app.state.teams.member_role(team, new).await.unwrap(),
        Some(TeamRole::Contributor)
    );
    // Removing membership remains effective across later logins.
    app.state.teams.remove_member(team, new).await.unwrap();
    app.login("new@example.com").await;
    assert!(app.state.teams.list_for_user(new).await.unwrap().is_empty());
}

#[tokio::test]
async fn manager_cannot_activate_a_pending_environment_owner() {
    let mut app = Fixture::spawn().await;
    app.create_user("manager@example.com", InstanceRole::Manager)
        .await;
    let owner = app
        .create_user("pending-owner@example.com", InstanceRole::Member)
        .await;
    app.state
        .users
        .update(
            owner,
            soika::ports::UserUpdate {
                status: Some(soika::domain::UserStatus::Pending),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    app.set_admin_users("pending-owner@example.com");
    let cookie = app.login("manager@example.com").await;
    let (status, _) = app
        .post_json(
            &format!("/api/admin/users/{owner}/approve"),
            &cookie,
            &json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        app.state
            .users
            .find_by_id(owner)
            .await
            .unwrap()
            .unwrap()
            .status
            .is_pending()
    );
}
