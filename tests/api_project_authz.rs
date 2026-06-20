//! Integration tests for project authorization on the JSON API.
//!
//! These drive the *real* application router in-process via
//! [`tower::ServiceExt::oneshot`] (no network, no containers), mirroring the
//! `team_access.rs` harness: a fresh `:memory:` SQLite DB is seeded, users are
//! created directly through the repository ports, and authenticated requests
//! replay the signed session cookie obtained from `/auth/login`.
//!
//! They pin the behaviour of the per-project role guards in
//! `src/api/projects.rs` (`effective_role` / `require_member` / `require_admin`)
//! and the API write handlers:
//!
//! - a direct `Role::Member` may `GET` a project but not `PATCH` it (admin-only);
//! - a direct project-admin membership row wins over the team-Member fallback;
//! - `POST /api/projects` to a team the caller is not in is `403`, and on
//!   success the creator becomes a project admin;
//! - `PATCH /api/projects/{id}` rejects a malformed `webhook_url` (`400`) and
//!   clears it when given an empty string;
//! - `POST /api/projects/{id}/regenerate-dsn` requires admin (member → `403`).

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::auth::hash_password;
use soika::config::LockoutConfig;
use soika::domain::{AuthProvider, Id, InstanceRole, Role, TeamRole, UserStatus};
use soika::ports::{NewProject, NewUser};
use soika::{AppState, Config, MIGRATOR, build_state, router};
use std::time::Duration;
use tower::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";

/// A created project's identifiers: the internal UUID (used to seed direct
/// membership rows) and the short public id (the wire/path param).
struct SeededProject {
    id: Id,
    short_id: String,
}

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

    async fn create_team(&self, name: &str) -> Id {
        self.state.teams.create(name.into()).await.unwrap().id
    }

    async fn create_project(&self, team_id: Id, slug: &str, dsn: &str) -> SeededProject {
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
        SeededProject {
            id: project.id,
            short_id: project.short_id,
        }
    }

    /// Grant `role` on a project under Variant A: project access is derived
    /// from membership in the owning team, so this resolves the project's team
    /// and writes the equivalent team role (`Role::Admin` → `TeamRole::Admin`,
    /// `Role::Member` → `TeamRole::Contributor`).
    async fn add_membership(&self, project_id: Id, user_id: Id, role: Role) {
        let team_id = self.team_of_project(project_id).await;
        let team_role = match role {
            Role::Admin => TeamRole::Admin,
            Role::Member => TeamRole::Contributor,
        };
        self.state
            .teams
            .add_member(team_id, user_id, team_role)
            .await
            .unwrap();
    }

    /// The effective project `Role` for a user, derived from the team role on
    /// the project's owning team (`TeamRole::Admin` → `Role::Admin`,
    /// `TeamRole::Contributor` → `Role::Member`).
    async fn membership_role(&self, project_id: Id, user_id: Id) -> Option<Role> {
        let team_id = self.team_of_project(project_id).await;
        self.state
            .teams
            .member_role(team_id, user_id)
            .await
            .unwrap()
            .map(|r| match r {
                TeamRole::Admin => Role::Admin,
                TeamRole::Contributor => Role::Member,
            })
    }

    /// Resolve the owning team id for an internal project id.
    async fn team_of_project(&self, project_id: Id) -> Id {
        self.state
            .projects
            .find_by_id(project_id)
            .await
            .unwrap()
            .expect("project exists")
            .team_id
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

    /// Authenticated `PATCH` with a JSON body, returning status + parsed body.
    async fn patch_json(&self, uri: &str, cookie: &str, body: &Value) -> (StatusCode, Value) {
        self.send_json("PATCH", uri, cookie, body).await
    }

    /// Authenticated `POST` with a JSON body, returning status + parsed body.
    async fn post_json(&self, uri: &str, cookie: &str, body: &Value) -> (StatusCode, Value) {
        self.send_json("POST", uri, cookie, body).await
    }

    /// Authenticated `POST` with no body (used by `regenerate-dsn`).
    async fn post_empty(&self, uri: &str, cookie: &str) -> (StatusCode, Value) {
        self.send("POST", uri, cookie, Body::empty()).await
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
// require_admin vs require_member: a direct Member may read but not write.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn direct_member_can_get_but_not_patch_project() {
    let app = Fixture::spawn().await;
    let user = app.create_user("member@example.com", false).await;
    let team = app.create_team("team-a").await;
    let project = app.create_project(team, "alpha", "dsn-alpha").await;
    // A direct project membership row at the lowest role.
    app.add_membership(project.id, user, Role::Member).await;
    let cookie = app.login("member@example.com").await;

    // GET is allowed for any role (require_member).
    let (status, body) = app
        .get(&format!("/api/projects/{}", project.short_id), &cookie)
        .await;
    assert_eq!(status, StatusCode::OK, "member may view the project");
    assert_eq!(body["id"], project.short_id);

    // PATCH is admin-only (require_admin) → 403 for a plain member.
    let (status, _) = app
        .patch_json(
            &format!("/api/projects/{}", project.short_id),
            &cookie,
            &json!({ "name": "renamed" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "member is forbidden from updating the project"
    );

    // The rejected PATCH did not mutate the project.
    let (_, body) = app
        .get(&format!("/api/projects/{}", project.short_id), &cookie)
        .await;
    assert_eq!(body["name"], "Project alpha", "name left unchanged");
}

// ---------------------------------------------------------------------------
// effective_role resolution (Variant A): a `TeamRole::Admin` of the owning team
// resolves to `Role::Admin`, granting admin-only project actions, whereas a
// plain `TeamRole::Contributor` would resolve only to `Role::Member`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn team_admin_grants_project_admin_access() {
    let app = Fixture::spawn().await;
    let user = app.create_user("admin-row@example.com", false).await;
    let team = app.create_team("team-a").await;
    let project = app.create_project(team, "alpha", "dsn-alpha").await;

    // The user is a Team Admin of the owning team; under Variant A this resolves
    // to Role::Admin on every project of that team.
    app.add_membership(project.id, user, Role::Admin).await;
    let cookie = app.login("admin-row@example.com").await;

    // An admin-only action (PATCH) succeeds, proving the Team Admin role
    // resolves to project-admin access.
    let (status, body) = app
        .patch_json(
            &format!("/api/projects/{}", project.short_id),
            &cookie,
            &json!({ "name": "renamed by admin" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a Team Admin resolves to project-admin access"
    );
    assert_eq!(body["name"], "renamed by admin");

    // The user remains a Team Admin → effective project Role::Admin.
    assert_eq!(
        app.membership_role(project.id, user).await,
        Some(Role::Admin),
        "the user remains a Team Admin"
    );
}

// ---------------------------------------------------------------------------
// POST /api/projects: team-membership gate + creator becomes Admin.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_project_in_foreign_team_is_forbidden() {
    let app = Fixture::spawn().await;
    let user = app.create_user("outsider@example.com", false).await;
    // The caller is a member of team-a, but tries to create in team-b.
    let team_a = app.create_team("team-a").await;
    let team_b = app.create_team("team-b").await;
    app.state
        .teams
        .add_member(team_a, user, TeamRole::Contributor)
        .await
        .unwrap();
    let cookie = app.login("outsider@example.com").await;

    let (status, _) = app
        .post_json(
            "/api/projects",
            &cookie,
            &json!({ "name": "Sneaky", "team_id": team_b.to_string() }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "non-member of the target team cannot create a project there"
    );
}

#[tokio::test]
async fn create_project_makes_creator_admin() {
    let app = Fixture::spawn().await;
    let user = app.create_user("creator@example.com", false).await;
    let team = app.create_team("team-a").await;
    // Under Variant A, creating a project requires being a Team Admin of the
    // target team (project rights derive from the owning team).
    app.state
        .teams
        .add_member(team, user, TeamRole::Admin)
        .await
        .unwrap();
    let cookie = app.login("creator@example.com").await;

    let (status, body) = app
        .post_json(
            "/api/projects",
            &cookie,
            &json!({ "name": "Fresh Project", "team_id": team.to_string() }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "team admin may create");

    // The newly-created project's short id is returned; resolve the internal id
    // through the port to confirm the creator's effective role.
    let short_id = body["id"].as_str().expect("created project id");
    let created = app
        .state
        .projects
        .find_by_short_id(short_id)
        .await
        .unwrap()
        .expect("created project exists");
    assert_eq!(
        app.membership_role(created.id, user).await,
        Some(Role::Admin),
        "creator has project-admin access via Team Admin role"
    );

    // End-to-end: the creator can now perform an admin-only action.
    let (status, _) = app
        .patch_json(
            &format!("/api/projects/{short_id}"),
            &cookie,
            &json!({ "muted": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "creator can administer the project");
}

// ---------------------------------------------------------------------------
// PATCH webhook_url: malformed → 400; empty string clears it.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn patch_rejects_bad_webhook_url_and_empty_string_clears_it() {
    let app = Fixture::spawn().await;
    let user = app.create_user("owner@example.com", false).await;
    let team = app.create_team("team-a").await;
    let project = app.create_project(team, "alpha", "dsn-alpha").await;
    app.add_membership(project.id, user, Role::Admin).await;
    let cookie = app.login("owner@example.com").await;
    let uri = format!("/api/projects/{}", project.short_id);

    // First set a valid webhook so we can observe it being cleared later.
    let (status, body) = app
        .patch_json(
            &uri,
            &cookie,
            &json!({ "webhook_url": "https://hooks.example.com/x" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["webhook_url"], "https://hooks.example.com/x");

    // A malformed URL is rejected at save time (400), leaving the prior value.
    let (status, _) = app
        .patch_json(&uri, &cookie, &json!({ "webhook_url": "not a url" }))
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "malformed webhook is rejected"
    );
    let (_, body) = app.get(&uri, &cookie).await;
    assert_eq!(
        body["webhook_url"], "https://hooks.example.com/x",
        "rejected PATCH did not change the stored webhook"
    );

    // An empty string clears the webhook (disables the channel) → null.
    let (status, body) = app
        .patch_json(&uri, &cookie, &json!({ "webhook_url": "" }))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["webhook_url"],
        Value::Null,
        "empty string clears the webhook"
    );
}

// ---------------------------------------------------------------------------
// regenerate-dsn requires admin.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn regenerate_dsn_requires_admin() {
    let app = Fixture::spawn().await;
    let member = app.create_user("member@example.com", false).await;
    let admin = app.create_user("admin@example.com", false).await;
    let team = app.create_team("team-a").await;
    let project = app.create_project(team, "alpha", "dsn-alpha").await;
    app.add_membership(project.id, member, Role::Member).await;
    app.add_membership(project.id, admin, Role::Admin).await;
    let uri = format!("/api/projects/{}/regenerate-dsn", project.short_id);

    // A plain member is forbidden.
    let member_cookie = app.login("member@example.com").await;
    let (status, _) = app.post_empty(&uri, &member_cookie).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "member cannot rotate the DSN key"
    );

    // The key was not rotated by the rejected request.
    let unchanged = app
        .state
        .projects
        .find_by_short_id(&project.short_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.dsn_public_key, "dsn-alpha");

    // An admin succeeds and the key actually changes.
    let admin_cookie = app.login("admin@example.com").await;
    let (status, body) = app.post_empty(&uri, &admin_cookie).await;
    assert_eq!(status, StatusCode::OK, "admin may rotate the DSN key");
    let rotated = body["public_key"].as_str().expect("public_key in response");
    assert_ne!(rotated, "dsn-alpha", "the DSN public key was rotated");
}

// ---------------------------------------------------------------------------
// PATCH /api/projects/{id}/team: move a project to another team (Manager-gated).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn manager_can_change_project_team_and_access_follows() {
    let app = Fixture::spawn().await;
    // An instance Owner (manager-capable) performs the move.
    app.create_user("owner@example.com", true).await;
    // A contributor of the *target* team gains access once the project moves.
    let target_member = app.create_user("target@example.com", false).await;

    let team_a = app.create_team("team-a").await;
    let team_b = app.create_team("team-b").await;
    let project = app.create_project(team_a, "alpha", "dsn-alpha").await;
    app.state
        .teams
        .add_member(team_b, target_member, TeamRole::Contributor)
        .await
        .unwrap();

    // Before the move, the target-team member has no access (closed membership).
    let target_cookie = app.login("target@example.com").await;
    let (status, _) = app
        .get(
            &format!("/api/projects/{}", project.short_id),
            &target_cookie,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "target-team member has no access before the move"
    );

    // The owner moves the project to team-b.
    let owner_cookie = app.login("owner@example.com").await;
    let (status, body) = app
        .patch_json(
            &format!("/api/projects/{}/team", project.short_id),
            &owner_cookie,
            &json!({ "team_id": team_b.to_string() }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "instance owner may move the project"
    );
    assert_eq!(
        body["team_id"],
        team_b.to_string(),
        "the response reflects the new owning team"
    );

    // The owning team really changed in storage.
    let moved = app
        .state
        .projects
        .find_by_short_id(&project.short_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(moved.team_id, team_b, "the project now belongs to team-b");

    // Access follows the team: the target-team member can now read the project.
    let (status, _) = app
        .get(
            &format!("/api/projects/{}", project.short_id),
            &target_cookie,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "access follows the owning team after the move"
    );
}

#[tokio::test]
async fn change_team_requires_manager() {
    let app = Fixture::spawn().await;
    // A team admin is project-admin, but NOT an instance manager.
    let admin = app.create_user("teamadmin@example.com", false).await;
    let team_a = app.create_team("team-a").await;
    let team_b = app.create_team("team-b").await;
    let project = app.create_project(team_a, "alpha", "dsn-alpha").await;
    app.add_membership(project.id, admin, Role::Admin).await;

    let cookie = app.login("teamadmin@example.com").await;
    let (status, _) = app
        .patch_json(
            &format!("/api/projects/{}/team", project.short_id),
            &cookie,
            &json!({ "team_id": team_b.to_string() }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a team admin (non-manager) cannot move a project between teams"
    );

    // The project did not move.
    let unchanged = app.team_of_project(project.id).await;
    assert_eq!(unchanged, team_a, "the project stayed in its team");
}

#[tokio::test]
async fn change_team_to_unknown_team_is_404() {
    let app = Fixture::spawn().await;
    app.create_user("owner@example.com", true).await;
    let team_a = app.create_team("team-a").await;
    let project = app.create_project(team_a, "alpha", "dsn-alpha").await;

    let cookie = app.login("owner@example.com").await;
    let (status, _) = app
        .patch_json(
            &format!("/api/projects/{}/team", project.short_id),
            &cookie,
            &json!({ "team_id": Id::new_v4().to_string() }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "moving to a non-existent team is a 404"
    );
    assert_eq!(
        app.team_of_project(project.id).await,
        team_a,
        "the project stayed put"
    );
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
