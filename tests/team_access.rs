//! Integration tests for team-scoped access control.
//!
//! Drive the real application router in-process via [`tower::ServiceExt::oneshot`]
//! (no network, no containers). A fresh `:memory:` SQLite DB is seeded with two
//! non-admin users and two teams — each owning one project — then a regular
//! member of `team-a` is logged in to prove the access rules end-to-end:
//!
//! - `GET /api/teams` lists only the teams the caller belongs to.
//! - `GET /api/teams/{id}` is `403` for a team the caller is not in.
//! - `GET /api/projects` lists only projects of the caller's teams.
//! - `GET /api/projects/{short_id}` is `403` for a project in a foreign team.
//!
//! An instance admin, by contrast, sees every team and project.

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

struct Fixture {
    router: Router,
    state: AppState,
}

struct Seeded {
    member_a: Id,
    team_a: Id,
    team_b: Id,
    project_a_short: String,
    project_b_short: String,
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

    /// Seed two non-admin users, two teams (one project each), and put
    /// `member_a` into `team-a` only.
    async fn seed(&self) -> Seeded {
        let member_a = self.create_user("a@example.com", false).await;
        let _member_b = self.create_user("b@example.com", false).await;

        let team_a = self.state.teams.create("team-a".into()).await.unwrap().id;
        let team_b = self.state.teams.create("team-b".into()).await.unwrap().id;

        let project_a = self.create_project(team_a, "alpha", "dsn-alpha").await;
        let project_b = self.create_project(team_b, "beta", "dsn-beta").await;

        self.state
            .teams
            .add_member(team_a, member_a, TeamRole::Contributor)
            .await
            .unwrap();

        Seeded {
            member_a,
            team_a,
            team_b,
            project_a_short: project_a,
            project_b_short: project_b,
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

    async fn create_project(&self, team_id: Id, slug: &str, dsn: &str) -> String {
        self.state
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
            .unwrap()
            .short_id
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
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .header("cookie", cookie)
            .body(Body::empty())
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
}

#[tokio::test]
async fn member_sees_only_their_teams_and_their_teams_projects() {
    let app = Fixture::spawn().await;
    let seeded = app.seed().await;
    let cookie = app.login("a@example.com").await;

    // /teams lists only team-a.
    let (status, teams) = app.get("/api/teams", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let teams = teams.as_array().expect("teams array");
    assert_eq!(teams.len(), 1, "member sees only their own team");
    assert_eq!(teams[0]["id"], seeded.team_a.to_string());

    // team-a is visible; team-b is forbidden.
    let (status, _) = app
        .get(&format!("/api/teams/{}", seeded.team_a), &cookie)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = app
        .get(&format!("/api/teams/{}", seeded.team_b), &cookie)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "foreign team is 403");

    // /projects lists only team-a's project — granted via team membership
    // alone, with no direct project membership row.
    let (status, projects) = app.get("/api/projects", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let projects = projects.as_array().expect("projects array");
    assert_eq!(projects.len(), 1, "member sees only their team's projects");

    // The team-a project detail is reachable; the team-b project is 403.
    let (status, _) = app
        .get(
            &format!("/api/projects/{}", seeded.project_a_short),
            &cookie,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "team project is viewable");
    let (status, _) = app
        .get(
            &format!("/api/projects/{}", seeded.project_b_short),
            &cookie,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "foreign project is 403");

    // Sanity: the seeded member id is the one we logged in as.
    assert_eq!(
        app.state
            .users
            .find_by_email("a@example.com")
            .await
            .unwrap()
            .unwrap()
            .id,
        seeded.member_a
    );
}

#[tokio::test]
async fn instance_admin_sees_every_team_and_project() {
    let app = Fixture::spawn().await;
    let _ = app.seed().await;
    app.create_user("admin@example.com", true).await;
    let cookie = app.login("admin@example.com").await;

    let (status, teams) = app.get("/api/teams", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(teams.as_array().unwrap().len(), 2, "admin sees all teams");

    let (status, projects) = app.get("/api/projects", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        projects.as_array().unwrap().len(),
        2,
        "admin sees all projects"
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
