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
