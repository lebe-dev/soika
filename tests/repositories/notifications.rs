use crate::fixtures::*;
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use soika::{
    alertmanager,
    domain::{config_file::ManagementSource, notifications::*},
};
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tower::ServiceExt;

struct Receiver {
    url: String,
    alerts: Arc<Mutex<Vec<AlertmanagerAlert>>>,
    fail: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Receiver {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Receiver {
    async fn start() -> Self {
        let alerts = Arc::new(Mutex::new(Vec::new()));
        let fail = Arc::new(AtomicBool::new(false));
        let (received, failing) = (alerts.clone(), fail.clone());
        let app = Router::new().route(
            "/prefix/api/v2/alerts",
            post(move |Json(batch): Json<Vec<AlertmanagerAlert>>| {
                let (received, failing) = (received.clone(), failing.clone());
                async move {
                    received.lock().unwrap().extend(batch);
                    if failing.load(Ordering::SeqCst) {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            url,
            alerts,
            fail,
            task,
        }
    }
    fn snapshot(&self) -> Vec<AlertmanagerAlert> {
        self.alerts.lock().unwrap().clone()
    }
}
async fn setup(db: &AppState, url: &str) -> (Project, Issue) {
    let project_id = seed_project(db).await;
    let project = db.projects.find_by_id(project_id).await.unwrap().unwrap();
    let issue = db
        .issues
        .upsert_by_fingerprint(IssueUpsert {
            project_id,
            fingerprint: "fp".into(),
            title: "Bad \"value\"\n{{ trigger }}".into(),
            culprit: None,
            level: Some("error".into()),
            environment: None,
            release: None,
            seen_at: chrono::Utc::now(),
        })
        .await
        .unwrap()
        .issue;
    let mut config = NotificationConfig::default();
    config.alertmanager.enabled = true;
    config.alertmanager.url = url.into();
    db.notifications.set(&config).await.unwrap();
    (project, issue)
}
async fn make_due(db: &AppState, issue_id: Id) {
    let mut saved = db
        .notifications
        .find_alert(issue_id)
        .await
        .unwrap()
        .unwrap();
    saved.last_sent_at = Some(db.clock.now() - chrono::Duration::days(1));
    db.notifications.save_alert(&saved).await.unwrap();
}
#[tokio::test]
async fn notifications_lifecycle_refresh_resolve_and_regression() {
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    alertmanager::refresh(&db).await.unwrap();
    assert!(receiver.snapshot().is_empty(), "no retroactive alerts");
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    let first = receiver.snapshot()[0].clone();
    assert_eq!(first.annotations["summary"], issue.title);
    assert_eq!(first.labels["issue_id"], issue.id.to_string());
    assert!(first.ends_at > first.starts_at);
    alertmanager::refresh(&db).await.unwrap();
    make_due(&db, issue.id).await;
    alertmanager::refresh(&db).await.unwrap();
    let second = receiver.snapshot()[1].clone();
    assert_eq!(first.labels, second.labels);
    assert_eq!(first.starts_at, second.starts_at);
    db.issues
        .set_status(issue.id, IssueStatus::Resolved)
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    let resolved = receiver.snapshot()[2].clone();
    assert_eq!(resolved.labels, first.labels);
    assert!(resolved.ends_at <= chrono::Utc::now());
    assert!(db.notifications.active("", 100).await.unwrap().is_empty());
    let regressed = db
        .issues
        .set_status(issue.id, IssueStatus::Unresolved)
        .await
        .unwrap();
    soika::notify::notify_regression(&db, &project, &regressed)
        .await
        .unwrap();
    assert_eq!(receiver.snapshot()[3].annotations["trigger"], "regression");
}

#[tokio::test]
async fn manual_reopen_restarts_alertmanager_delivery() {
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    let uri = |action| format!("/api/issues/{}/{action}", issue.short_id);
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "POST",
        &uri("resolve"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    alertmanager::refresh(&db).await.unwrap();
    assert!(
        db.notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_none()
    );
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "POST",
        &uri("unresolve"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let alerts = receiver.snapshot();
    assert_eq!(alerts.len(), 3);
    assert_eq!(alerts[2].annotations["trigger"], "regression");
    assert!(alerts[2].ends_at > db.clock.now());
    assert!(
        db.notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_some()
    );
    make_due(&db, issue.id).await;
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(receiver.snapshot().len(), 4);
    // Repeating unresolve must not restart an already active alert.
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "POST",
        &uri("unresolve"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receiver.snapshot().len(), 4);
}

#[tokio::test]
async fn env_project_labels_apply_to_delivery_and_refresh_identity() {
    let mut db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    let path = std::env::temp_dir().join(format!("soika-labels-{}.yaml", Id::new_v4()));
    let yaml = |team: &str| {
        format!(
            "version: 1\nnotifications:\n  alertmanager:\n    project_labels:\n      p: {{team: {team}, severity: warning}}\n"
        )
    };
    std::fs::write(&path, yaml("first")).unwrap();
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Env;
    Arc::make_mut(&mut db.config).management.path = path.to_str().unwrap().into();
    Arc::make_mut(&mut db.config).alertmanager_url = Some(receiver.url.clone());
    let db = soika::config_sync::synchronize(db).await.unwrap();
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    let first = receiver.snapshot()[0].clone();
    assert_eq!(first.labels["team"], "first");
    assert_eq!(first.labels["severity"], "warning");
    assert_eq!(first.labels["project"], "p");
    std::fs::write(&path, yaml("second")).unwrap();
    let db = soika::config_sync::synchronize(db).await.unwrap();
    alertmanager::refresh(&db).await.unwrap();
    let alerts = receiver.snapshot();
    assert_eq!(alerts[1].labels, first.labels);
    assert!(alerts[1].ends_at <= chrono::Utc::now());
    assert_eq!(alerts[2].labels["team"], "second");
    std::fs::remove_file(path).unwrap();
}
#[tokio::test]
async fn notifications_retry_failed_delivery_and_close_snapshot_after_template_change() {
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    receiver.fail.store(true, Ordering::SeqCst);
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    assert_eq!(db.notifications.active("", 10).await.unwrap().len(), 1);
    receiver.fail.store(false, Ordering::SeqCst);
    alertmanager::refresh(&db).await.unwrap();
    let mut config = db.notifications.get().await.unwrap();
    config
        .alertmanager
        .labels
        .insert("routing".into(), "new".into());
    db.notifications.set(&config).await.unwrap();
    alertmanager::refresh(&db).await.unwrap();
    let alerts = receiver.snapshot();
    assert!(alerts[2].ends_at <= chrono::Utc::now());
    assert!(!alerts[2].labels.contains_key("routing"));
    assert_eq!(alerts[3].labels["routing"], "new");
    db.issues.delete(issue.id).await.unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert!(db.notifications.active("", 10).await.unwrap().is_empty());
}
#[tokio::test]
async fn notifications_respect_project_and_issue_mute_independently_of_smtp() {
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (mut project, mut issue) = setup(&db, &receiver.url).await;
    project.muted = true;
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    project.muted = false;
    issue.status = IssueStatus::Muted;
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    assert!(receiver.snapshot().is_empty());
    issue.status = IssueStatus::Unresolved;
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    db.issues
        .mute(issue.id, MuteSpec::Forever, chrono::Utc::now())
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert!(receiver.snapshot()[1].ends_at <= chrono::Utc::now());
}

async fn request(
    db: &AppState,
    role: InstanceRole,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let user = db
        .users
        .create(NewUser {
            email: format!("{}@test.home", Id::new_v4()),
            display_name: "Tester".into(),
            password_hash: "hash".into(),
            instance_role: role,
            auth_provider: AuthProvider::Local,
            status: UserStatus::Active,
        })
        .await
        .unwrap();
    let session = db
        .sessions
        .create(
            Id::new_v4().to_string(),
            user.id,
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .await
        .unwrap();
    let cookie = soika::auth::build_session_cookie(&db.config.secret_key, &session.id, false);
    let response = soika::router::build(db.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("cookie", cookie.split(';').next().unwrap())
                .header("x-soika-request", "1")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    (status, value)
}
#[tokio::test]
async fn notifications_api_authorization_yaml_guard_validation_and_preview() {
    let mut db = notification_db().await;
    let path = "/api/settings/notifications";
    let config = serde_json::to_value(NotificationConfig::default()).unwrap();
    assert_eq!(
        request(
            &db,
            InstanceRole::Member,
            "GET",
            path,
            serde_json::Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&db, InstanceRole::Manager, "PUT", path, config.clone())
            .await
            .0,
        StatusCode::OK
    );
    let mut invalid = config.clone();
    invalid["alertmanager"]["annotations"]["summary"] = "{{ secret }}".into();
    assert_eq!(
        request(&db, InstanceRole::Owner, "PUT", path, invalid)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    Arc::make_mut(&mut db.config).management.notification_source = ManagementSource::Yaml;
    assert_eq!(
        request(&db, InstanceRole::Owner, "PUT", path, config.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, view) = request(
        &db,
        InstanceRole::Manager,
        "GET",
        path,
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["source"], "yaml");
    let (status, preview) = request(
        &db,
        InstanceRole::Manager,
        "POST",
        &format!("{path}/preview"),
        config,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        preview["annotations"]["summary"],
        "ValueError: example error"
    );
    assert!(db.notifications.active("", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn notifications_environment_default_ui_override_and_clear() {
    let mut db = notification_db().await;
    let receiver = Receiver::start().await;
    Arc::make_mut(&mut db.config).alertmanager_url = Some(receiver.url.clone());
    let (project, issue) = setup(&db, "http://explicit:9093").await;
    let mut config = db.notifications.get().await.unwrap();
    config.alertmanager.url.clear();
    let (status, view) = request(
        &db,
        InstanceRole::Manager,
        "PUT",
        "/api/settings/notifications",
        serde_json::to_value(&config).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["default_url"], receiver.url);
    assert_eq!(view["config"]["alertmanager"]["url"], "");
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    assert_eq!(receiver.snapshot().len(), 1);
    assert_eq!(db.notifications.get().await.unwrap().alertmanager.url, "");
    // Changing the environment default at restart migrates alerts because no
    // explicit override was stored by the UI.
    let next = Receiver::start().await;
    Arc::make_mut(&mut db.config).alertmanager_url = Some(next.url.clone());
    alertmanager::refresh(&db).await.unwrap();
    assert!(receiver.snapshot()[1].ends_at <= chrono::Utc::now());
    assert_eq!(next.snapshot().len(), 1);
    config.alertmanager.url = receiver.url.clone();
    db.notifications.set(&config).await.unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert!(next.snapshot()[1].ends_at <= chrono::Utc::now());
    assert_eq!(receiver.snapshot().len(), 3);
}

#[tokio::test]
async fn notifications_resume_saved_alerts_after_rebuilding_application_state() {
    let config = (*notification_db().await.config).clone();
    let pool = crate::common::test_db().await;
    let db = soika::build_state(pool.clone(), config.clone()).unwrap();
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    drop(db);
    let restarted = soika::build_state(pool, config).unwrap();
    alertmanager::refresh(&restarted).await.unwrap();
    assert_eq!(
        receiver.snapshot().len(),
        1,
        "restart preserves last successful send time"
    );
    make_due(&restarted, issue.id).await;
    alertmanager::refresh(&restarted).await.unwrap();
    assert_eq!(receiver.snapshot()[0].labels, receiver.snapshot()[1].labels);
    assert_eq!(
        receiver.snapshot()[0].starts_at,
        receiver.snapshot()[1].starts_at
    );
    restarted
        .issues
        .set_status(issue.id, IssueStatus::Resolved)
        .await
        .unwrap();
    receiver.fail.store(true, Ordering::SeqCst);
    alertmanager::refresh(&restarted).await.unwrap();
    assert!(
        restarted
            .notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_some()
    );
    receiver.fail.store(false, Ordering::SeqCst);
    alertmanager::refresh(&restarted).await.unwrap();
    assert!(
        restarted
            .notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_none()
    );
}

async fn notification_db() -> AppState {
    let mut db = test_db().await;
    Arc::make_mut(&mut db.config).notifications_alertmanager_enabled = true;
    db
}

#[tokio::test]
async fn notifications_feature_flag_blocks_delivery_refresh_and_writes() {
    let mut db = test_db().await;
    assert!(
        !db.config.notifications_alertmanager_enabled,
        "feature is off by default"
    );
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    assert!(receiver.snapshot().is_empty());
    assert!(
        db.notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_none()
    );
    let path = "/api/settings/notifications";
    let (status, view) = request(
        &db,
        InstanceRole::Manager,
        "GET",
        path,
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["feature_enabled"], false);
    let config = serde_json::to_value(db.notifications.get().await.unwrap()).unwrap();
    for (method, uri) in [
        ("PUT", path.to_string()),
        ("POST", format!("{path}/preview")),
    ] {
        let (status, error) = request(&db, InstanceRole::Owner, method, &uri, config.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("NOTIFICATIONS_ALERTMANAGER_ENABLED")
        );
    }
    Arc::make_mut(&mut db.config).notifications_alertmanager_enabled = true;
    soika::notify::notify_new_issue(&db, &project, &issue)
        .await
        .unwrap();
    assert_eq!(receiver.snapshot().len(), 1);
    Arc::make_mut(&mut db.config).notifications_alertmanager_enabled = false;
    db.issues
        .set_status(issue.id, IssueStatus::Resolved)
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(
        receiver.snapshot().len(),
        1,
        "flag also blocks existing refresh/resolution requests"
    );
    assert!(
        db.notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_some()
    );
    Arc::make_mut(&mut db.config).notifications_alertmanager_enabled = true;
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(receiver.snapshot().len(), 2);
    assert!(
        db.notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn notifications_test_alert_permissions_localization_and_failures() {
    let mut db = notification_db().await;
    let receiver = Receiver::start().await;
    setup(&db, &receiver.url).await;
    let path = "/api/settings/notifications/test";
    let body = serde_json::json!({"locale": "ru"});
    assert_eq!(
        request(&db, InstanceRole::Member, "POST", path, body.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut config = db.notifications.get().await.unwrap();
    config.alertmanager.url.clear();
    db.notifications.set(&config).await.unwrap();
    Arc::make_mut(&mut db.config).alertmanager_url = Some(receiver.url.clone());
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", path, body.clone())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    Arc::make_mut(&mut db.config).management.notification_source = ManagementSource::Yaml;
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", path, body.clone())
            .await
            .0,
        StatusCode::OK
    );
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Env;
    let mut config = db.notifications.get().await.unwrap();
    config.alertmanager.enabled = false;
    db.notifications.set(&config).await.unwrap();
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", path, body.clone())
            .await
            .0,
        StatusCode::OK
    );
    config.alertmanager.enabled = true;
    db.notifications.set(&config).await.unwrap();
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Ui;
    Arc::make_mut(&mut db.config).management.notification_source = ManagementSource::Ui;
    config.alertmanager.url = receiver.url.clone();
    db.notifications.set(&config).await.unwrap();
    for (locale, title) in [
        ("ru", "Тестовое уведомление Soika"),
        ("en", "Soika test notification"),
    ] {
        assert_eq!(
            request(
                &db,
                InstanceRole::Manager,
                "POST",
                path,
                serde_json::json!({"locale": locale})
            )
            .await
            .0,
            StatusCode::OK
        );
        let alert = receiver.snapshot().last().unwrap().clone();
        assert_eq!(alert.annotations["summary"], title);
        assert_eq!(alert.labels["alertname"], "SoikaTestAlert");
        assert_eq!((alert.ends_at - alert.starts_at).num_seconds(), 300);
    }
    assert!(db.notifications.active("", 10).await.unwrap().is_empty());
    receiver.fail.store(true, Ordering::SeqCst);
    let (status, error) = request(&db, InstanceRole::Owner, "POST", path, body.clone()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error["error"], "Alertmanager returned HTTP 503");
    let mut config = db.notifications.get().await.unwrap();
    config.alertmanager.enabled = false;
    db.notifications.set(&config).await.unwrap();
    receiver.fail.store(false, Ordering::SeqCst);
    assert_eq!(
        request(&db, InstanceRole::Owner, "POST", path, body.clone())
            .await
            .0,
        StatusCode::OK
    );
    Arc::make_mut(&mut db.config).notifications_alertmanager_enabled = false;
    assert_eq!(
        request(&db, InstanceRole::Owner, "POST", path, body)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(receiver.snapshot().len(), 6);
}

#[tokio::test]
async fn notifications_environment_service_settings_override_database_and_block_admin_edits() {
    let mut db = notification_db().await;
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Env;
    Arc::make_mut(&mut db.config).management.path = "/nonexistent-config.yaml".into();
    let receiver = Receiver::start().await;
    Arc::make_mut(&mut db.config).alertmanager_url = Some(receiver.url.clone());
    Arc::make_mut(&mut db.config).organization_name = "Environment organization".into();
    db.settings
        .set_org_name("Database organization".into())
        .await
        .unwrap();
    let default_team = insert_team(&db, "Environment default team").await;
    Arc::make_mut(&mut db.config).management.default_team_ids = vec![default_team];
    let db = soika::config_sync::synchronize(db).await.unwrap();
    assert_eq!(
        db.settings.default_team_ids().await.unwrap(),
        vec![default_team]
    );
    assert_eq!(
        db.settings.get().await.unwrap().org_name,
        "Environment organization"
    );
    assert!(db.notifications.get().await.unwrap().alertmanager.enabled);
    assert_eq!(
        db.notifications.get().await.unwrap().alertmanager.url,
        receiver.url
    );
    let (status, view) = request(
        &db,
        InstanceRole::Owner,
        "GET",
        "/api/settings/notifications",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["source"], "env");
    assert_eq!(
        db.settings.get().await.unwrap().allow_signup,
        db.config.allow_signup
    );
    for role in [InstanceRole::Owner, InstanceRole::Manager] {
        for body in [
            serde_json::json!({"allow_signup": true}),
            serde_json::json!({"default_team_ids": []}),
        ] {
            assert_eq!(
                request(&db, role, "PATCH", "/api/settings", body).await.0,
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            request(
                &db,
                role,
                "PATCH",
                "/api/settings",
                serde_json::json!({"org_name": "Changed"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &db,
                role,
                "PUT",
                "/api/settings/notifications",
                serde_json::to_value(NotificationConfig::default()).unwrap()
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &db,
                role,
                "POST",
                "/api/settings/notifications/test",
                serde_json::json!({"locale":"en"})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(receiver.snapshot().len(), 2);
    assert_eq!(
        db.settings.get().await.unwrap().org_name,
        "Environment organization"
    );
}

#[tokio::test]
async fn notifications_view_reports_tls_verification_setting() {
    let mut db = test_db().await;
    for skip_tls in [false, true] {
        Arc::make_mut(&mut db.config).alertmanager_skip_tls = skip_tls;
        let (status, view) = request(
            &db,
            InstanceRole::Manager,
            "GET",
            "/api/settings/notifications",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["alertmanager_skip_tls"], skip_tls);
    }
}

#[tokio::test]
async fn notification_intervals_throttle_refresh_and_close_without_waiting() {
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    db.notifications
        .set_interval(None, Some(600))
        .await
        .unwrap();
    db.notifications
        .set_interval(Some(project.id), Some(120))
        .await
        .unwrap();
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    let first = receiver.snapshot()[0].clone();
    assert_eq!((first.ends_at - first.starts_at).num_seconds(), 360);
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(receiver.snapshot().len(), 1, "not due yet");
    let mut saved = db
        .notifications
        .find_alert(issue.id)
        .await
        .unwrap()
        .unwrap();
    saved.last_sent_at = Some(db.clock.now() - chrono::Duration::seconds(121));
    db.notifications.save_alert(&saved).await.unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(
        receiver.snapshot().len(),
        2,
        "project override is due before global interval"
    );
    db.notifications
        .set_interval(Some(project.id), None)
        .await
        .unwrap();
    let view = db.notifications.interval(Some(project.id)).await.unwrap();
    assert_eq!(view.interval_seconds, None);
    assert_eq!(view.effective_interval_seconds, 600);
    alertmanager::refresh(&db).await.unwrap();
    let extended = receiver.snapshot().last().unwrap().clone();
    assert_eq!(
        receiver.snapshot().len(),
        3,
        "longer interval extends the short lease immediately"
    );
    assert!((extended.ends_at - db.clock.now()).num_seconds() >= 1799);
    db.issues
        .set_status(issue.id, IssueStatus::Resolved)
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(
        receiver.snapshot().len(),
        4,
        "resolution ignores refresh interval"
    );
    assert!(
        db.notifications
            .find_alert(issue.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn notification_interval_api_uses_service_ownership_and_validates_bounds() {
    let mut db = notification_db().await;
    let (status, _) = request(
        &db,
        InstanceRole::Manager,
        "PUT",
        "/api/settings/notifications/interval",
        serde_json::json!({"interval_seconds": 300}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        db.notifications
            .interval(None)
            .await
            .unwrap()
            .effective_interval_seconds,
        300
    );
    for seconds in [0, 9, 86401] {
        let (status, _) = request(
            &db,
            InstanceRole::Owner,
            "PUT",
            "/api/settings/notifications/interval",
            serde_json::json!({"interval_seconds": seconds}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = request(
        &db,
        InstanceRole::Member,
        "PUT",
        "/api/settings/notifications/interval",
        serde_json::json!({"interval_seconds": 60}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Env;
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        "/api/settings/notifications/interval",
        serde_json::json!({"interval_seconds": 90}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "global interval is env-managed"
    );
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Ui;
    Arc::make_mut(&mut db.config).management.project_source = ManagementSource::Yaml;
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        "/api/settings/notifications/interval",
        serde_json::json!({"interval_seconds": 60}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "YAML project ownership does not lock global defaults"
    );
}

#[tokio::test]
async fn project_interval_api_overrides_inherits_and_enforces_project_access() {
    let mut db = notification_db().await;
    let project = seed_project(&db).await;
    let slug = db.projects.find_by_id(project).await.unwrap().unwrap().slug;
    let uri = format!("/api/projects/{slug}/alertmanager-interval");
    let (status, view) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        &uri,
        serde_json::json!({"interval_seconds": 120}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["effective_interval_seconds"], 120);
    let (status, _) = request(
        &db,
        InstanceRole::Member,
        "PUT",
        &uri,
        serde_json::json!({"interval_seconds": 180}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &db,
        InstanceRole::Member,
        "GET",
        &uri,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &db,
        InstanceRole::Manager,
        "PUT",
        &uri,
        serde_json::json!({"interval_seconds": -1}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, view) = request(
        &db,
        InstanceRole::Manager,
        "PUT",
        &uri,
        serde_json::json!({"interval_seconds": null}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(view["interval_seconds"].is_null());
    assert_eq!(view["effective_interval_seconds"], 60);
    Arc::make_mut(&mut db.config).management.project_source = ManagementSource::Yaml;
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        &uri,
        serde_json::json!({"interval_seconds": 180}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(&db, InstanceRole::Owner, "GET", &uri, serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn project_severity_overrides_labels_and_closes_the_previous_alert_identity() {
    use soika::domain::notifications::AlertSeverity;
    let db = notification_db().await;
    db.notifications.set_severity(None, None).await.unwrap();
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    assert_eq!(receiver.snapshot()[0].labels["severity"], "error");
    db.alertmanager
        .set_project_labels(std::collections::BTreeMap::from([(
            project.slug.clone(),
            std::collections::BTreeMap::from([("severity".into(), "info".into())]),
        )]))
        .await;
    db.notifications
        .set_severity(Some(project.id), Some(AlertSeverity::Critical))
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    let alerts = receiver.snapshot();
    assert_eq!(alerts.len(), 3);
    assert_eq!(alerts[1].labels["severity"], "error");
    assert!(alerts[1].ends_at <= db.clock.now());
    assert_eq!(alerts[2].labels["severity"], "critical");
    db.notifications
        .set_severity(Some(project.id), None)
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    let alerts = receiver.snapshot();
    assert_eq!(alerts[3].labels["severity"], "critical");
    assert!(alerts[3].ends_at <= db.clock.now());
    assert_eq!(
        alerts[4].labels["severity"], "info",
        "clearing restores the legacy project label"
    );
}

#[tokio::test]
async fn project_severity_api_validates_classes_and_honors_project_ownership() {
    let mut db = notification_db().await;
    let project_id = seed_project(&db).await;
    let project = db.projects.find_by_id(project_id).await.unwrap().unwrap();
    let uri = format!("/api/projects/{}/alertmanager-severity", project.slug);
    for severity in ["critical", "warning", "info"] {
        let (status, view) = request(
            &db,
            InstanceRole::Owner,
            "PUT",
            &uri,
            serde_json::json!({"severity": severity}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["severity"], severity);
    }
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        &uri,
        serde_json::json!({"severity": "urgent"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        &db,
        InstanceRole::Member,
        "PUT",
        &uri,
        serde_json::json!({"severity": "critical"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &db,
        InstanceRole::Member,
        "GET",
        &uri,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, view) = request(
        &db,
        InstanceRole::Manager,
        "PUT",
        &uri,
        serde_json::json!({"severity": null}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(view["severity"].is_null());
    Arc::make_mut(&mut db.config).management.project_source = ManagementSource::Yaml;
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        &uri,
        serde_json::json!({"severity": "info"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(&db, InstanceRole::Owner, "GET", &uri, serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn severity_selects_alert_name_and_resolves_previous_identity() {
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    let mut config = db.notifications.get().await.unwrap();
    config
        .alertmanager
        .labels
        .insert("alertname".into(), "SoikaIssue".into());
    db.notifications.set(&config).await.unwrap();

    for (index, (severity, name)) in [
        (AlertSeverity::Critical, "SoikaIssueCritical"),
        (AlertSeverity::Warning, "SoikaIssueWarning"),
        (AlertSeverity::Info, "SoikaIssueInfo"),
    ]
    .into_iter()
    .enumerate()
    {
        db.notifications
            .set_severity(Some(project.id), Some(severity))
            .await
            .unwrap();
        if index == 0 {
            alertmanager::notify(&db, &project, &issue, "new_issue")
                .await
                .unwrap();
        } else {
            let previous = receiver.snapshot().last().unwrap().clone();
            alertmanager::refresh(&db).await.unwrap();
            let alerts = receiver.snapshot();
            let resolved = &alerts[alerts.len() - 2];
            assert_eq!(resolved.labels, previous.labels);
            assert!(resolved.ends_at <= db.clock.now());
        }
        let alerts = receiver.snapshot();
        assert_eq!(alerts.len(), index * 2 + 1);
        let alert = alerts.last().unwrap();
        assert_eq!(alert.labels["severity"], severity.as_str());
        assert_eq!(alert.labels["alertname"], name);
        assert!(alert.ends_at > db.clock.now());
    }
}

#[tokio::test]
async fn env_severity_names_apply_to_delivery_and_refresh() {
    let mut db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    let config = Arc::make_mut(&mut db.config);
    config.management.service_source = ManagementSource::Env;
    config.alertmanager_severity_critical = "CustomCritical".into();
    config.alertmanager_severity_warning = "CustomWarning".into();
    config.alertmanager_severity_info = "CustomInfo".into();
    // The global critical default is inherited for the initial send.
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    assert_eq!(receiver.snapshot()[0].labels["alertname"], "CustomCritical");
    for (severity, name) in [
        (AlertSeverity::Warning, "CustomWarning"),
        (AlertSeverity::Info, "CustomInfo"),
    ] {
        db.notifications
            .set_severity(Some(project.id), Some(severity))
            .await
            .unwrap();
        let previous = receiver.snapshot().last().unwrap().clone();
        alertmanager::refresh(&db).await.unwrap();
        let alerts = receiver.snapshot();
        let resolved = &alerts[alerts.len() - 2];
        assert_eq!(resolved.labels, previous.labels);
        assert!(resolved.ends_at <= db.clock.now());
        let alert = alerts.last().unwrap();
        assert_eq!(alert.labels["alertname"], name);
        assert_eq!(alert.labels["severity"], severity.as_str());
    }
    // Project test names remain independent of severity name overrides.
    alertmanager::send_project_test(&db, &project, soika::domain::Locale::En)
        .await
        .unwrap();
    assert_eq!(
        receiver.snapshot().last().unwrap().labels["alertname"],
        "SoikaIssueCritical"
    );
}

#[tokio::test]
async fn alert_names_api_uses_service_ownership_and_updates_live_alerts() {
    let mut db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    let uri = "/api/settings/notifications/names";
    let body = serde_json::json!({
        "critical": " CustomCritical ", "warning": "CustomWarning",
        "info": "CustomInfo", "project_test": "CustomTest"
    });
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    assert_eq!(
        request(&db, InstanceRole::Member, "PUT", uri, body.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    // Label-template ownership does not prevent editing service-owned names.
    Arc::make_mut(&mut db.config).management.notification_source = ManagementSource::Yaml;
    let (status, saved) = request(&db, InstanceRole::Manager, "PUT", uri, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["critical"], "CustomCritical");
    assert_eq!(
        serde_json::to_value(db.notifications.names().await.unwrap()).unwrap(),
        saved
    );
    let (_, view) = request(
        &db,
        InstanceRole::Owner,
        "GET",
        "/api/settings/notifications",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(view["names"], saved);
    let (status, preview) = request(
        &db,
        InstanceRole::Owner,
        "POST",
        "/api/settings/notifications/preview",
        serde_json::to_value(db.notifications.get().await.unwrap()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["labels"]["alertname"], "CustomCritical");

    for (severity, name) in [
        (AlertSeverity::Critical, "CustomCritical"),
        (AlertSeverity::Warning, "CustomWarning"),
        (AlertSeverity::Info, "CustomInfo"),
    ] {
        db.notifications
            .set_severity(Some(project.id), Some(severity))
            .await
            .unwrap();
        let previous = receiver.snapshot().last().unwrap().clone();
        alertmanager::refresh(&db).await.unwrap();
        let alerts = receiver.snapshot();
        assert_eq!(alerts[alerts.len() - 2].labels, previous.labels);
        assert!(alerts[alerts.len() - 2].ends_at <= db.clock.now());
        assert_eq!(alerts.last().unwrap().labels["alertname"], name);
    }
    alertmanager::send_project_test(&db, &project, soika::domain::Locale::En)
        .await
        .unwrap();
    assert_eq!(
        receiver.snapshot().last().unwrap().labels["alertname"],
        "CustomTest"
    );
    for key in ["critical", "warning", "info", "project_test"] {
        let mut invalid = body.clone();
        invalid[key] = serde_json::json!("   ");
        assert_eq!(
            request(&db, InstanceRole::Owner, "PUT", uri, invalid)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        serde_json::to_value(db.notifications.names().await.unwrap()).unwrap(),
        saved
    );
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Env;
    assert_eq!(
        request(&db, InstanceRole::Owner, "PUT", uri, body.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, view) = request(
        &db,
        InstanceRole::Owner,
        "GET",
        "/api/settings/notifications",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(view["names"]["critical"], "SoikaIssueCritical");
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Ui;
    assert_eq!(
        serde_json::to_value(alertmanager::effective_names(&db).await.unwrap()).unwrap(),
        saved
    );
    Arc::make_mut(&mut db.config).notifications_alertmanager_enabled = false;
    assert_eq!(
        request(&db, InstanceRole::Owner, "PUT", uri, body).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn global_alertmanager_severity_is_inherited_and_project_override_wins() {
    use soika::domain::notifications::AlertSeverity;
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    db.notifications
        .set_severity(None, Some(AlertSeverity::Warning))
        .await
        .unwrap();
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    assert_eq!(receiver.snapshot()[0].labels["severity"], "warning");
    assert_eq!(
        receiver.snapshot()[0].labels["alertname"],
        "SoikaIssueWarning"
    );
    db.notifications
        .set_severity(Some(project.id), Some(AlertSeverity::Critical))
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(receiver.snapshot()[2].labels["severity"], "critical");
    db.notifications
        .set_severity(Some(project.id), None)
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(receiver.snapshot()[4].labels["severity"], "warning");
    db.notifications.set_severity(None, None).await.unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(receiver.snapshot()[6].labels["severity"], "error");
}

#[tokio::test]
async fn default_severity_api_follows_service_ownership() {
    let mut db = notification_db().await;
    Arc::make_mut(&mut db.config).management.project_source = ManagementSource::Yaml;
    let uri = "/api/settings/notifications/severity";
    let (status, view) = request(
        &db,
        InstanceRole::Manager,
        "PUT",
        uri,
        serde_json::json!({"severity": "info"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["severity"], "info");
    let (status, _) = request(
        &db,
        InstanceRole::Member,
        "PUT",
        uri,
        serde_json::json!({"severity": "critical"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        uri,
        serde_json::json!({"severity": "urgent"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Env;
    let (status, _) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        uri,
        serde_json::json!({"severity": "critical"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn initial_ui_default_severity_is_critical_and_can_be_changed() {
    let db = notification_db().await;
    assert_eq!(
        db.notifications.severity(None).await.unwrap(),
        Some(AlertSeverity::Critical)
    );
    let receiver = Receiver::start().await;
    let (project, issue) = setup(&db, &receiver.url).await;
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    assert_eq!(
        receiver.snapshot()[0].labels["severity"],
        "critical",
        "global DB default overrides the event level"
    );
    db.notifications
        .set_severity(None, Some(AlertSeverity::Warning))
        .await
        .unwrap();
    let db = soika::config_sync::synchronize(db).await.unwrap();
    assert_eq!(
        db.notifications.severity(None).await.unwrap(),
        Some(AlertSeverity::Warning),
        "UI choice survives synchronization"
    );
}

#[tokio::test]
async fn notifications_project_test_uses_project_context_and_enforces_gates() {
    let mut db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, _) = setup(&db, &receiver.url).await;
    let path = format!("/api/projects/{}/alertmanager-test", project.slug);
    let body = serde_json::json!({"locale": "en"});
    db.notifications
        .set_severity(Some(project.id), Some(AlertSeverity::Warning))
        .await
        .unwrap();
    db.alertmanager
        .set_project_labels(BTreeMap::from([(
            project.slug.clone(),
            BTreeMap::from([("chat".into(), "project-chat".into())]),
        )]))
        .await;
    assert_eq!(
        request(&db, InstanceRole::Member, "POST", &path, body.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", &path, body.clone())
            .await
            .0,
        StatusCode::OK
    );
    let alert = receiver.snapshot().pop().unwrap();
    assert_eq!(alert.labels["project_id"], project.id.to_string());
    assert_eq!(alert.labels["project"], project.slug);
    assert_eq!(alert.labels["severity"], "warning");
    assert_eq!(alert.labels["chat"], "project-chat");
    assert_eq!(alert.labels["test"], "true");
    assert_eq!(alert.labels["alertname"], "SoikaIssueCritical");
    assert_eq!(
        alert.annotations["summary"],
        format!("Test alert: {}", project.name)
    );
    assert_eq!(alert.annotations["trigger"], "test");
    db.notifications
        .set_severity(None, Some(AlertSeverity::Info))
        .await
        .unwrap();
    db.notifications
        .set_severity(Some(project.id), None)
        .await
        .unwrap();
    assert_eq!(
        request(
            &db,
            InstanceRole::Manager,
            "POST",
            &path,
            serde_json::json!({"locale": "ru"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let inherited = receiver.snapshot().pop().unwrap();
    assert_eq!(inherited.labels["severity"], "info");
    assert_eq!(
        inherited.annotations["summary"],
        format!("Тестовый алерт: {}", project.name)
    );
    assert!(
        alert
            .generator_url
            .ends_with(&format!("/projects/{}", project.slug))
    );
    assert!(db.notifications.active("", 10).await.unwrap().is_empty());
    let mut names = db.notifications.names().await.unwrap();
    names.project_test = "CustomProjectTest".into();
    db.notifications.set_names(&names).await.unwrap();
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", &path, body.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        receiver.snapshot().last().unwrap().labels["alertname"],
        "CustomProjectTest"
    );
    receiver.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", &path, body.clone())
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    receiver.fail.store(false, Ordering::SeqCst);
    let mut config = db.notifications.get().await.unwrap();
    config.alertmanager.enabled = false;
    db.notifications.set(&config).await.unwrap();
    assert_eq!(
        request(
            &db,
            InstanceRole::Manager,
            "GET",
            &path,
            serde_json::Value::Null
        )
        .await
        .1["enabled"],
        false
    );
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", &path, body.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    config.alertmanager.enabled = true;
    db.notifications.set(&config).await.unwrap();
    Arc::make_mut(&mut db.config).notifications_alertmanager_enabled = false;
    assert_eq!(
        request(&db, InstanceRole::Manager, "POST", &path, body)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn event_severity_mode_maps_levels_and_keeps_peak_across_refresh() {
    let db = notification_db().await;
    let receiver = Receiver::start().await;
    let (project, _) = setup(&db, &receiver.url).await;
    db.notifications
        .set_severity_settings(None, Some(SeverityMode::Event), Some(AlertSeverity::Info))
        .await
        .unwrap();
    let mut input = IssueUpsert {
        project_id: project.id,
        fingerprint: "severity-peak".into(),
        title: "Peak".into(),
        culprit: None,
        level: Some("warning".into()),
        environment: None,
        release: None,
        seen_at: db.clock.now(),
    };
    let issue = db
        .issues
        .upsert_by_fingerprint(input.clone())
        .await
        .unwrap()
        .issue;
    alertmanager::notify(&db, &project, &issue, "new_issue")
        .await
        .unwrap();
    assert_eq!(receiver.snapshot()[0].labels["severity"], "warning");
    input.level = Some("error".into());
    db.issues
        .upsert_by_fingerprint(input.clone())
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    let alerts = receiver.snapshot();
    assert_eq!(alerts.len(), 3);
    assert!(alerts[1].ends_at <= db.clock.now());
    assert_eq!(alerts[2].labels["severity"], "critical");
    assert_eq!(alerts[2].labels["alertname"], "SoikaIssueCritical");
    input.level = Some("debug".into());
    db.issues.upsert_by_fingerprint(input).await.unwrap();
    make_due(&db, issue.id).await;
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(
        receiver.snapshot().last().unwrap().labels["severity"],
        "critical"
    );
    db.notifications
        .set_severity_settings(
            Some(project.id),
            Some(SeverityMode::Fixed),
            Some(AlertSeverity::Info),
        )
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(
        receiver.snapshot().last().unwrap().labels["severity"],
        "info"
    );
    db.notifications
        .set_severity_settings(Some(project.id), None, None)
        .await
        .unwrap();
    alertmanager::refresh(&db).await.unwrap();
    assert_eq!(
        receiver.snapshot().last().unwrap().labels["severity"],
        "critical"
    );
}

#[tokio::test]
async fn event_severity_mode_api_validates_mode_and_ownership() {
    let mut db = notification_db().await;
    let uri = "/api/settings/notifications/severity";
    let (status, view) = request(
        &db,
        InstanceRole::Owner,
        "PUT",
        uri,
        serde_json::json!({"mode": "event", "severity": "warning"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["mode"], "event");
    assert_eq!(
        db.notifications.severity_mode(None).await.unwrap(),
        Some(SeverityMode::Event)
    );
    assert_eq!(
        request(
            &db,
            InstanceRole::Owner,
            "PUT",
            uri,
            serde_json::json!({"mode": "invalid", "severity": "warning"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    Arc::make_mut(&mut db.config).management.service_source = ManagementSource::Env;
    assert_eq!(
        request(
            &db,
            InstanceRole::Owner,
            "PUT",
            uri,
            serde_json::json!({"mode": "fixed", "severity": "critical"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn event_severity_mode_covers_mapping_unknown_fallback_and_merge() {
    let db = notification_db().await;
    let project_id = seed_project(&db).await;
    db.notifications
        .set_severity_settings(
            None,
            Some(SeverityMode::Event),
            Some(AlertSeverity::Warning),
        )
        .await
        .unwrap();
    let mut ids = Vec::new();
    for (index, (level, expected)) in [
        (Some("fatal"), AlertSeverity::Critical),
        (Some("error"), AlertSeverity::Critical),
        (Some("warning"), AlertSeverity::Warning),
        (Some("info"), AlertSeverity::Info),
        (Some("debug"), AlertSeverity::Info),
        (Some("unknown"), AlertSeverity::Warning),
        (None, AlertSeverity::Warning),
    ]
    .into_iter()
    .enumerate()
    {
        let issue = db
            .issues
            .upsert_by_fingerprint(IssueUpsert {
                project_id,
                fingerprint: format!("mapped-{index}"),
                title: "Mapped".into(),
                culprit: None,
                level: level.map(String::from),
                environment: None,
                release: None,
                seen_at: db.clock.now(),
            })
            .await
            .unwrap()
            .issue;
        assert_eq!(
            alertmanager::effective_severity(&db, Some(project_id), Some(issue.id))
                .await
                .unwrap(),
            Some(expected)
        );
        ids.push(issue.id);
    }
    db.notifications
        .set_severity_settings(None, Some(SeverityMode::Event), None)
        .await
        .unwrap();
    assert_eq!(
        alertmanager::effective_severity(&db, Some(project_id), Some(ids[6]))
            .await
            .unwrap(),
        Some(AlertSeverity::Critical)
    );
    // The info target inherits the critical source's peak when fingerprints merge.
    let merged = db
        .issues
        .override_fingerprint(ids[3], "mapped-0".into())
        .await
        .unwrap();
    assert_eq!(
        merged.level.as_deref(),
        Some("info"),
        "preserve the original SDK level"
    );
    assert_eq!(
        db.notifications.event_severity(merged.id).await.unwrap(),
        Some(AlertSeverity::Critical)
    );
    // Fixed overrides and clearing the override preserve global event-mode inheritance.
    db.notifications
        .set_severity_settings(
            Some(project_id),
            Some(SeverityMode::Fixed),
            Some(AlertSeverity::Info),
        )
        .await
        .unwrap();
    assert_eq!(
        alertmanager::effective_severity(&db, Some(project_id), Some(merged.id))
            .await
            .unwrap(),
        Some(AlertSeverity::Info)
    );
    db.notifications
        .set_severity_settings(Some(project_id), None, None)
        .await
        .unwrap();
    assert_eq!(
        alertmanager::effective_severity(&db, Some(project_id), Some(merged.id))
            .await
            .unwrap(),
        Some(AlertSeverity::Critical)
    );
    db.issues
        .set_status(merged.id, IssueStatus::Resolved)
        .await
        .unwrap();
    let regression = db
        .issues
        .upsert_by_fingerprint(IssueUpsert {
            project_id,
            fingerprint: "mapped-0".into(),
            title: "Regression".into(),
            culprit: None,
            level: Some("debug".into()),
            environment: None,
            release: None,
            seen_at: db.clock.now(),
        })
        .await
        .unwrap();
    assert!(regression.is_regression);
    assert_eq!(
        db.notifications.event_severity(merged.id).await.unwrap(),
        Some(AlertSeverity::Critical)
    );
}

#[tokio::test]
async fn project_event_severity_api_roundtrip_and_yaml_lock() {
    let mut db = notification_db().await;
    let id = seed_project(&db).await;
    let slug = db.projects.find_by_id(id).await.unwrap().unwrap().slug;
    let uri = format!("/api/projects/{slug}/alertmanager-severity");
    let body = serde_json::json!({"mode": "event", "severity": "info"});
    let (status, view) = request(&db, InstanceRole::Owner, "PUT", &uri, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view, body);
    let (_, view) = request(
        &db,
        InstanceRole::Owner,
        "GET",
        &uri,
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(view, body);
    Arc::make_mut(&mut db.config).management.project_source = ManagementSource::Yaml;
    assert_eq!(
        request(&db, InstanceRole::Owner, "PUT", &uri, body).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        db.notifications.severity_mode(Some(id)).await.unwrap(),
        Some(SeverityMode::Event)
    );
}
