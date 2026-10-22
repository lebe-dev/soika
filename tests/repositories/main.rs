//! Repository contract tests: every port is exercised through `AppState`, so
//! the same suite runs against whichever backend `SOIKA_TEST_DB` selects
//! (SQLite by default, PostgreSQL via `just test-postgres`).
//!
//! Fixtures seed data through the ports too — no backend-specific SQL — which
//! keeps the suite honest about what each adapter actually guarantees.

#[path = "../common/mod.rs"]
mod common;

mod events;
mod favorites;
mod invites;
mod issues;
mod mute_rules;
mod passkeys;
mod projects;
mod sessions;
mod settings;
mod teams;
mod users;

mod fixtures {
    use std::time::Duration;

    pub use soika::AppState;
    pub use soika::Error;
    pub use soika::config::WritePolicy;
    pub use soika::domain::*;
    pub use soika::ports::*;

    use soika::config::LockoutConfig;
    use soika::{Config, build_state};

    /// A fresh database on the selected backend, wired into every repository.
    pub async fn test_db() -> AppState {
        test_db_with_write_policy(WritePolicy::default()).await
    }

    /// Like [`test_db`], with a custom write retry / delete-batch policy for the
    /// repositories on the ingest / retention path (issues, events).
    pub async fn test_db_with_write_policy(write: WritePolicy) -> AppState {
        let mut config = test_config();
        config.db.write = write;
        build_state(crate::common::test_db().await, config).expect("build state")
    }

    /// Create a user through the port and return its id.
    pub async fn insert_user(db: &AppState, email: &str) -> Id {
        db.users
            .create(NewUser {
                email: email.to_string(),
                display_name: email.to_string(),
                password_hash: "hash".to_string(),
                instance_role: InstanceRole::Member,
                auth_provider: AuthProvider::Local,
                status: UserStatus::Active,
            })
            .await
            .expect("insert user")
            .id
    }

    /// Create a team through the port and return its id.
    pub async fn insert_team(db: &AppState, name: &str) -> Id {
        db.teams
            .create(name.to_string())
            .await
            .expect("insert team")
            .id
    }

    /// Create a project (named after its slug) through the port and return its id.
    pub async fn insert_project(db: &AppState, team_id: Id, slug: &str, dsn: &str) -> Id {
        db.projects
            .create(NewProject {
                team_id,
                name: slug.to_string(),
                slug: slug.to_string(),
                dsn_public_key: dsn.to_string(),
                retention_events: 1000,
                retention_days: 0,
                webhook_url: None,
            })
            .await
            .expect("insert project")
            .id
    }

    /// A team plus one project in it; returns the project id.
    pub async fn seed_project(db: &AppState) -> Id {
        let team_id = insert_team(db, "t").await;
        insert_project(db, team_id, "p", "dsn-p").await
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
            admin_enabled: true,
            admin_users: Default::default(),
            default_events_retention: 1000,
            default_retention_days: 0,
            retention_cron: "0 0 * * * *".into(),
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
}
