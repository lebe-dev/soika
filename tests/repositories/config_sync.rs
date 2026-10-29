use crate::fixtures::*;
use soika::domain::config_file::*;
fn settings() -> ManagementSettings {
    ManagementSettings {
        project_source: ManagementSource::Yaml,
        team_source: ManagementSource::Yaml,
        ..Default::default()
    }
}
fn desired() -> ConfigFile {
    parse("version: 1\nteams:\n  - name: Platform\n    members: []\nprojects:\n  - slug: api\n    name: API\n    team: Platform\n", &settings()).unwrap()
}
#[tokio::test]
async fn sync_create_idempotence_orphan_restore_and_ui() {
    let db = test_db().await;
    let file = desired();
    let settings = settings();
    let first = db.config_sync.apply(&file, &settings, 1000).await.unwrap();
    assert_eq!(first.created, 2);
    let project = db.projects.find_by_slug("api").await.unwrap().unwrap();
    db.projects
        .update(
            project.id,
            ProjectUpdate {
                muted: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let before = db.projects.find_by_id(project.id).await.unwrap().unwrap();
    let again = db.config_sync.apply(&file, &settings, 1000).await.unwrap();
    assert_eq!(again.unchanged, 2);
    assert_eq!(
        db.projects
            .find_by_id(project.id)
            .await
            .unwrap()
            .unwrap()
            .updated_at,
        before.updated_at
    );
    let mut empty = file.clone();
    empty.projects = Some(vec![]);
    db.config_sync.apply(&empty, &settings, 1000).await.unwrap();
    assert!(
        db.projects
            .find_by_id(project.id)
            .await
            .unwrap()
            .unwrap()
            .orphaned_at
            .is_some()
    );
    db.config_sync.apply(&file, &settings, 1000).await.unwrap();
    let restored = db.projects.find_by_id(project.id).await.unwrap().unwrap();
    assert!(restored.orphaned_at.is_none());
    assert_eq!(restored.dsn_public_key, project.dsn_public_key);
    assert!(restored.muted);
    db.config_sync.apply(&empty, &settings, 1000).await.unwrap();
    db.config_sync
        .apply(&ConfigFile::default(), &ManagementSettings::default(), 1000)
        .await
        .unwrap();
    assert!(
        db.projects
            .find_by_id(project.id)
            .await
            .unwrap()
            .unwrap()
            .orphaned_at
            .is_none()
    );
}
#[tokio::test]
async fn invalid_team_reference_rolls_back_all_changes() {
    let db = test_db().await;
    let mut file = desired();
    file.projects.as_mut().unwrap()[0].team = "missing".into();
    assert!(
        db.config_sync
            .apply(&file, &settings(), 1000)
            .await
            .is_err()
    );
    assert!(db.teams.list().await.unwrap().is_empty());
}
#[tokio::test]
async fn orphan_team_deletion_cannot_delete_owned_projects() {
    let db = test_db().await;
    db.config_sync
        .apply(&desired(), &settings(), 1000)
        .await
        .unwrap();
    let mut empty = desired();
    empty.teams = Some(vec![]);
    empty.projects = Some(vec![]);
    let mut config = settings();
    config.team_orphan_policy = OrphanPolicy::Delete;
    assert!(db.config_sync.apply(&empty, &config, 1000).await.is_err());
    assert_eq!(db.projects.list().await.unwrap().len(), 1);
    config.project_orphan_policy = OrphanPolicy::Delete;
    db.config_sync.apply(&empty, &config, 1000).await.unwrap();
    assert!(db.projects.list().await.unwrap().is_empty());
    assert!(db.teams.list().await.unwrap().is_empty());
}
#[tokio::test]
async fn email_and_group_grants_use_maximum_role_and_remove_revoked_memberships() {
    let db = test_db().await;
    let user = insert_user(&db, "alice@example.com").await;
    let mut file = desired();
    let team = &mut file.teams.as_mut().unwrap()[0];
    team.members = Some(vec![EmailGrant {
        email: "ALICE@example.com".into(),
        role: TeamRole::Contributor,
    }]);
    team.sso_groups = Some(vec![GroupGrant {
        group: "ACME/Platform".into(),
        role: TeamRole::Admin,
    }]);
    db.config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    let id = db.teams.list().await.unwrap()[0].id;
    assert_eq!(
        db.teams.member_role(id, user).await.unwrap(),
        Some(TeamRole::Contributor)
    );
    db.config_sync
        .reconcile_user(user, &["acme/platform".into()])
        .await
        .unwrap();
    assert_eq!(
        db.teams.member_role(id, user).await.unwrap(),
        Some(TeamRole::Admin)
    );
    db.config_sync
        .reconcile_user(user, &["acme/platform/subgroup".into()])
        .await
        .unwrap();
    assert_eq!(
        db.teams.member_role(id, user).await.unwrap(),
        Some(TeamRole::Contributor)
    );
    file.teams.as_mut().unwrap()[0].members = Some(vec![]);
    db.config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    assert_eq!(db.teams.member_role(id, user).await.unwrap(), None);
}

#[tokio::test]
async fn management_sources_are_independent_and_ingestion_checks_orphans() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    for (project_source, team_source) in [
        (ManagementSource::Ui, ManagementSource::Ui),
        (ManagementSource::Yaml, ManagementSource::Ui),
        (ManagementSource::Ui, ManagementSource::Yaml),
        (ManagementSource::Yaml, ManagementSource::Yaml),
    ] {
        let mut db = test_db().await;
        Arc::make_mut(&mut db.config).management = ManagementSettings {
            project_source,
            team_source,
            ..Default::default()
        };
        let owner = db
            .users
            .create(NewUser {
                email: "owner@example.com".into(),
                display_name: "Owner".into(),
                password_hash: "hash".into(),
                instance_role: InstanceRole::Owner,
                auth_provider: AuthProvider::Local,
                status: UserStatus::Active,
            })
            .await
            .unwrap();
        let team = insert_team(&db, "Platform").await;
        let project_id = insert_project(&db, team, "api", "test-dsn").await;
        let project = db.projects.find_by_id(project_id).await.unwrap().unwrap();
        let (_, cookie) =
            soika::auth::start_session(&*db.sessions, &*db.clock, &db.config, owner.id)
                .await
                .unwrap();
        let router = soika::router::build(db.clone());
        for (uri, method, body, blocked) in [
            (
                "/api/projects".to_string(),
                "POST",
                serde_json::json!({"name":"other","team_id":team}),
                project_source == ManagementSource::Yaml,
            ),
            (
                format!("/api/projects/{}", project.short_id),
                "PATCH",
                serde_json::json!({"name":"renamed"}),
                project_source == ManagementSource::Yaml,
            ),
            (
                "/api/teams".to_string(),
                "POST",
                serde_json::json!({"name":"Other"}),
                team_source == ManagementSource::Yaml,
            ),
            (
                format!("/api/teams/{team}"),
                "PATCH",
                serde_json::json!({"name":"renamed-team"}),
                team_source == ManagementSource::Yaml,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .method(method)
                        .header("cookie", &cookie)
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            if blocked {
                assert_eq!(response.status(), StatusCode::FORBIDDEN);
            } else {
                assert!(response.status().is_success());
            }
        }
        let settings = ManagementSettings {
            project_source: ManagementSource::Yaml,
            ..Default::default()
        };
        db.config_sync
            .apply(
                &ConfigFile {
                    version: 1,
                    projects: Some(vec![]),
                    teams: None,
                },
                &settings,
                1000,
            )
            .await
            .unwrap();
        let request = Request::builder()
            .uri(format!("/api/{project_id}/store/?sentry_key=test-dsn"))
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn startup_reads_file_only_for_yaml_sources() {
    use std::sync::Arc;
    let mut state = test_db().await;
    Arc::make_mut(&mut state.config).management.path =
        format!("/private/tmp/soika-absent-{}.yaml", Id::new_v4());
    assert!(soika::config_sync::synchronize(state.clone()).await.is_ok());
    Arc::make_mut(&mut state.config).management.project_source = ManagementSource::Yaml;
    assert!(soika::config_sync::synchronize(state).await.is_err());
}

#[tokio::test]
async fn concurrent_synchronizations_do_not_duplicate_entities() {
    let db = test_db().await;
    let file = desired();
    let settings = settings();
    let (a, b) = tokio::join!(
        db.config_sync.apply(&file, &settings, 1000),
        db.config_sync.apply(&file, &settings, 1000)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.created + b.created, 2);
    assert_eq!(a.unchanged + b.unchanged, 2);
    assert_eq!(db.projects.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn declarative_teams_suspend_default_memberships_without_erasing_preferences() {
    let ui = test_db().await;
    let mut config = (*ui.config).clone();
    config.management.team_source = ManagementSource::Yaml;
    let db = soika::build_state(crate::common::test_db().await, config).unwrap();
    let team = insert_team(&db, "Platform").await;
    db.settings.set_default_team_ids(vec![team]).await.unwrap();
    let user = insert_user(&db, "new@example.com").await;
    assert_eq!(db.teams.member_role(team, user).await.unwrap(), None);
    let file = ConfigFile {
        version: 1,
        projects: None,
        teams: Some(vec![TeamConfig {
            name: "Platform".into(),
            members: Some(vec![]),
            sso_groups: None,
        }]),
    };
    db.config_sync
        .apply(&file, &db.config.management, 1000)
        .await
        .unwrap();
    assert_eq!(db.settings.default_team_ids().await.unwrap(), vec![team]);
}

struct ConfigFixture(std::path::PathBuf);
impl ConfigFixture {
    fn new(text: &str) -> Self {
        let path = std::env::temp_dir().join(format!("soika-config-{}.yaml", Id::new_v4()));
        std::fs::write(&path, text).unwrap();
        Self(path)
    }
    fn configure(&self, state: &mut AppState, settings: ManagementSettings) {
        std::sync::Arc::make_mut(&mut state.config).management = ManagementSettings {
            path: self.0.to_str().unwrap().into(),
            ..settings
        };
    }
}
impl Drop for ConfigFixture {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}

#[tokio::test]
async fn startup_expands_yaml_scalars_and_ignores_ui_sections() {
    let file = ConfigFixture::new(
        "version: 1\nprojects: [{slug: api, name: '${PATH}', team: Platform}]\nteams: '${SOIKA_TEST_UNSET_IGNORED_SECTION}'\n",
    );
    let mut state = test_db().await;
    insert_team(&state, "Platform").await;
    file.configure(
        &mut state,
        ManagementSettings {
            project_source: ManagementSource::Yaml,
            ..Default::default()
        },
    );
    let state = soika::config_sync::synchronize(state).await.unwrap();
    assert_eq!(
        state
            .projects
            .find_by_slug("api")
            .await
            .unwrap()
            .unwrap()
            .name,
        std::env::var("PATH").unwrap()
    );
    assert!(state.sync_report.loaded);
    assert!(!state.sync_report.default_file);
    assert_eq!(state.sync_report.created, 1);
}

#[tokio::test]
async fn startup_rejects_invalid_or_unexpandable_yaml_and_unsupported_groups() {
    for text in [
        "version: [",
        "version: 1\nprojects: [{slug: api, name: '${SOIKA_TEST_UNSET_CONFIG_VALUE}', team: Platform}]\nteams: []",
        "version: 1\nprojects: [{slug: api, name: '${UNFINISHED', team: Platform}]\nteams: []",
        "version: 1\nprojects: []\nteams: [{name: Platform, sso_groups: [{group: acme, role: admin}]}]",
    ] {
        let file = ConfigFixture::new(text);
        let mut state = test_db().await;
        file.configure(&mut state, settings());
        assert!(
            soika::config_sync::synchronize(state.clone())
                .await
                .is_err()
        );
        assert!(state.teams.list().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn startup_reports_the_shipped_default_file() {
    let file = ConfigFixture::new(soika::config_sync::DEFAULT_FILE);
    let mut state = test_db().await;
    file.configure(&mut state, settings());
    let state = soika::config_sync::synchronize(state).await.unwrap();
    assert!(state.sync_report.default_file);
    assert_eq!(state.sync_report.projects_in_file, 0);
    assert_eq!(state.sync_report.teams_in_file, 0);
}

#[tokio::test]
async fn sync_updates_transfers_and_restores_teams_and_reports_grant_bindings() {
    let db = test_db().await;
    let local = insert_user(&db, "local@example.com").await;
    let mut file = desired();
    db.config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    let before = db.projects.find_by_slug("api").await.unwrap().unwrap();
    file.teams.as_mut().unwrap().push(TeamConfig {
        name: "New".into(),
        members: Some(vec![]),
        sso_groups: None,
    });
    let project = &mut file.projects.as_mut().unwrap()[0];
    project.name = "Renamed".into();
    project.team = "New".into();
    project.retention_events = Some(50);
    project.retention_days = Some(7);
    project.webhook_url = Some("https://example.com/hook".into());
    file.teams.as_mut().unwrap()[1].members = Some(vec![
        EmailGrant {
            email: "LOCAL@example.com".into(),
            role: TeamRole::Admin,
        },
        EmailGrant {
            email: "pending@example.com".into(),
            role: TeamRole::Contributor,
        },
    ]);
    let report = db
        .config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    assert_eq!(report.updated, 1);
    assert_eq!(report.pending_grants, 1);
    assert_eq!(report.local_grants, 1);
    assert_eq!(report.local_accounts[0].user_id, Some(local));
    let after = db.projects.find_by_slug("api").await.unwrap().unwrap();
    assert_eq!(before.id, after.id);
    assert_eq!(before.dsn_public_key, after.dsn_public_key);
    assert_eq!(after.name, "Renamed");
    assert_eq!(after.retention_events, 50);
    assert_eq!(after.retention_days, 7);
    let new_team = db.teams.find_by_id(after.team_id).await.unwrap().unwrap();
    assert_eq!(new_team.name, "New");
    let grants = db.config_sync.inspect_team(after.team_id).await.unwrap();
    assert_eq!(grants.members.len(), 2);
    assert_eq!(grants.sources.len(), 1);
    assert!(
        grants
            .members
            .iter()
            .any(|g| g.user_id == Some(local) && g.local_account)
    );
    // Removing a declaration freezes team access; restoring it reapplies grants.
    file.teams
        .as_mut()
        .unwrap()
        .retain(|t| t.name != "Platform");
    let orphaned = db
        .config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    assert_eq!(orphaned.orphaned_teams, 1);
    let again = db
        .config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    assert_eq!(again.orphaned, 0);
    file.teams.as_mut().unwrap().push(TeamConfig {
        name: "Platform".into(),
        members: Some(vec![]),
        sso_groups: None,
    });
    let restored = db
        .config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    assert_eq!(restored.restored, 1);
    file.teams.as_mut().unwrap()[0].members.as_mut().unwrap()[0].role = TeamRole::Contributor;
    let changed = db
        .config_sync
        .apply(&file, &settings(), 1000)
        .await
        .unwrap();
    assert_eq!(changed.updated, 1);
    assert_eq!(
        db.teams.member_role(after.team_id, local).await.unwrap(),
        Some(TeamRole::Contributor)
    );
}
