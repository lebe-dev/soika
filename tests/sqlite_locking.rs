//! Integration tests for the SQLite write path under lock contention.
//!
//! SQLite serializes writers on a single lock, so a single-binary tracker that
//! ingests events while the retention sweep deletes rows *will* contend. These
//! tests drive the real router against a **file-backed** database (a `:memory:`
//! database has one connection and cannot contend) and pin down the contract:
//!
//!   * concurrent ingestion + a retention sweep never fails a request;
//!   * a write blocked by a foreign writer waits and then succeeds (`BEGIN
//!     IMMEDIATE` + `busy_timeout` + the adapter's bounded retries);
//!   * once that budget is spent, ingestion answers `429` + `Retry-After` — not
//!     a `500`, so the SDK re-sends the event instead of dropping it.
//!
//! The failure mode being guarded against: a deferred `BEGIN` (sqlx's default)
//! starts as a reader and gets `SQLITE_BUSY` ("database is locked") the instant
//! it tries to upgrade to a writer, with `busy_timeout` powerless to help.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use soika::config::{DbConfig, Synchronous, WritePolicy};
use soika::domain::{Id, Project};
use soika::ports::{IssueFilter, NewProject};
use soika::{AppState, Config, MIGRATOR, build_state, router, scheduler};
use sqlx::SqlitePool;
use tower::ServiceExt; // for `oneshot`

const DSN_KEY: &str = "locking-test-key";

/// A temporary database file, removed (with its `-wal` / `-shm` sidecars) when
/// the test ends. WAL needs a real file, so `:memory:` is not an option here.
struct TempDb {
    path: std::path::PathBuf,
}

impl TempDb {
    fn new() -> TempDb {
        let path = std::env::temp_dir().join(format!("soika-locking-{}.db", Id::new_v4()));
        TempDb { path }
    }

    fn url(&self) -> String {
        format!("sqlite://{}", self.path.display())
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let path = format!("{}{suffix}", self.path.display());
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A running application under test, backed by a real database file.
struct TestApp {
    router: Router,
    state: AppState,
    project: Project,
    db: TempDb,
}

impl TestApp {
    /// Build a file-backed app whose pool is configured exactly like production
    /// (WAL, busy timeout, retries), with `db` letting a test tighten the knobs.
    async fn spawn(db_config: DbConfig) -> TestApp {
        let db = TempDb::new();
        let pool = soika::connect_pool(&db.url(), &db_config)
            .await
            .expect("open pool");
        MIGRATOR.run(&pool).await.expect("run migrations");

        let state = build_state(pool, test_config(&db.url(), db_config)).expect("build state");

        let team = state
            .teams
            .create("locking-team".into())
            .await
            .expect("create team");
        let project = state
            .projects
            .create(NewProject {
                team_id: team.id,
                name: "Locking".into(),
                slug: "locking".into(),
                dsn_public_key: DSN_KEY.into(),
                // A tight count retention so the concurrent sweep actually
                // deletes (and therefore actually takes the write lock).
                retention_events: 5,
                retention_days: 0,
                webhook_url: None,
            })
            .await
            .expect("create project");

        let router = router::build(state.clone());
        TestApp {
            router,
            state,
            project,
            db,
        }
    }

    /// A second, independent pool onto the same file — the stand-in for a foreign
    /// writer (another process, or a `sqlite3` shell) holding the write lock.
    async fn foreign_pool(&self) -> SqlitePool {
        soika::connect_pool(&self.db.url(), &DbConfig::default())
            .await
            .expect("open second pool")
    }

    /// POST one envelope carrying `event_id`; returns status + `Retry-After`.
    async fn ingest(&self, event_id: &str) -> (StatusCode, Option<String>) {
        let uri = format!("/api/{}/envelope/", self.project.id);
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("x-sentry-auth", sentry_auth(DSN_KEY))
            .header("content-type", "application/x-sentry-envelope")
            .body(Body::from(envelope_body(event_id)))
            .expect("build request");

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router response");
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        // Drain the body so the response is fully consumed.
        let _ = to_bytes(response.into_body(), usize::MAX).await;
        (status, retry_after)
    }
}

/// Concurrent ingestion of many events, with the retention sweep running against
/// the same database, must not fail a single request.
///
/// This is the regression test for `ingest failed to process event` /
/// `(code: 5) database is locked`: with a deferred `BEGIN` in the issue upsert,
/// tasks racing on the same fingerprint fail here immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_ingestion_and_retention_never_fail_a_request() {
    let app = Arc::new(TestApp::spawn(DbConfig::default()).await);

    // Every event carries the same exception, so all tasks collide on the *same*
    // issue row — the read-then-write a deferred transaction cannot survive.
    const TASKS: usize = 8;
    const PER_TASK: usize = 12;

    let sweeper = {
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            // Sweep repeatedly for the duration of the ingest burst; each sweep
            // deletes everything past the project's 5-event retention.
            for _ in 0..20 {
                scheduler::run_retention(&app.state)
                    .await
                    .expect("retention sweep");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };

    let mut ingesters = Vec::with_capacity(TASKS);
    for task in 0..TASKS {
        let app = Arc::clone(&app);
        ingesters.push(tokio::spawn(async move {
            for i in 0..PER_TASK {
                let event_id = format!("{:032x}", task * PER_TASK + i);
                let (status, _) = app.ingest(&event_id).await;
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "task {task} event {i} was rejected with {status}"
                );
            }
        }));
    }

    for handle in ingesters {
        handle.await.expect("ingest task");
    }
    sweeper.await.expect("sweeper task");

    // Every event was grouped: one issue, counting all of them. The counter is
    // authoritative even though retention pruned the stored event rows.
    let issues = app
        .state
        .issues
        .list(app.project.id, IssueFilter::default())
        .await
        .expect("list issues");
    assert_eq!(issues.len(), 1, "all events share one fingerprint");
    assert_eq!(issues[0].event_count, (TASKS * PER_TASK) as i64);
}

/// A write blocked by a foreign writer waits for the lock and then succeeds:
/// `BEGIN IMMEDIATE` makes `busy_timeout` applicable, and the adapter's retries
/// cover whatever is left.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ingestion_waits_out_a_foreign_writer_and_succeeds() {
    let app = TestApp::spawn(DbConfig::default()).await;
    let foreign = app.foreign_pool().await;

    // Hold the write lock from outside the application pool.
    let mut tx = foreign
        .begin_with("BEGIN IMMEDIATE")
        .await
        .expect("take the write lock");
    sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(Id::new_v4().to_string())
        .bind("blocker")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&mut *tx)
        .await
        .expect("write inside the blocking tx");

    let releaser = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        tx.commit().await.expect("release the write lock");
    });

    let (status, _) = app.ingest("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").await;
    releaser.await.expect("releaser task");

    assert_eq!(
        status,
        StatusCode::OK,
        "the write should have waited for the lock, not failed"
    );
}

/// With the waiting budget removed (no busy timeout, no retries), a blocked write
/// surfaces as `429` + `Retry-After` rather than `500`, so the SDK holds the
/// event and re-sends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exhausted_budget_answers_429_with_retry_after() {
    let db_config = DbConfig {
        // No waiting anywhere: SQLite returns busy immediately and the adapter
        // does not retry, which is the "budget spent" state a long outage would
        // produce.
        busy_timeout: Duration::ZERO,
        busy_retry_after: Duration::from_secs(7),
        synchronous: Synchronous::Normal,
        write: WritePolicy {
            max_retries: 0,
            ..WritePolicy::default()
        },
        ..DbConfig::default()
    };
    let app = TestApp::spawn(db_config).await;
    let foreign = app.foreign_pool().await;

    let mut tx = foreign
        .begin_with("BEGIN IMMEDIATE")
        .await
        .expect("take the write lock");
    sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(Id::new_v4().to_string())
        .bind("blocker")
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .execute(&mut *tx)
        .await
        .expect("write inside the blocking tx");

    let (status, retry_after) = app.ingest("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").await;

    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "expected backpressure"
    );
    assert_eq!(retry_after.as_deref(), Some("7"));

    tx.commit().await.expect("release the write lock");
}

/// A minimal config: file-backed db, no SMTP (NoopMailer), signup disabled.
fn test_config(database_url: &str, db: DbConfig) -> Config {
    Config {
        organization_name: "test".into(),
        database_url: database_url.to_string(),
        db,
        bind_addr: "127.0.0.1:0".into(),
        base_url: "http://localhost".into(),
        secret_key: "test-secret-key".into(),
        allow_signup: false,
        // Match the project's retention so the sweep prunes on every pass.
        default_events_retention: 5,
        default_retention_days: 0,
        retention_cron: "0 0 * * * *".into(),
        smtp: None,
        oidc: None,
        sentry: None,
        lockout: Default::default(),
        timezone: "UTC".to_string(),
    }
}

fn sentry_auth(key: &str) -> String {
    format!("Sentry sentry_version=7, sentry_key={key}, sentry_client=test/1.0")
}

/// One envelope with a single `event` item. All events share one exception type
/// (and therefore one fingerprint) so writers contend on the same issue row.
fn envelope_body(event_id: &str) -> Vec<u8> {
    let payload: Value = json!({
        "event_id": event_id,
        "level": "error",
        "exception": {
            "values": [{
                "type": "ContentionError",
                "value": "boom",
                "stacktrace": {
                    "frames": [{
                        "function": "handler",
                        "module": "app.web",
                        "filename": "app/web.py",
                        "lineno": 42,
                        "in_app": true
                    }]
                }
            }]
        }
    });
    let header = json!({ "event_id": event_id });
    let item_header = json!({ "type": "event" });
    let payload = serde_json::to_string(&payload).expect("serialize payload");
    format!("{header}\n{item_header}\n{payload}\n").into_bytes()
}
