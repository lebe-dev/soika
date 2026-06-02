//! Integration tests for Sentry-compatible event ingestion.
//!
//! These drive the *real* application router in-process via
//! [`tower::ServiceExt::oneshot`] — no network, no containers. Each test builds
//! a fresh `:memory:` SQLite database, runs the migrations, seeds a project with
//! a known DSN public key, then POSTs raw bodies at the two ingestion wire
//! formats:
//!
//!   * modern  — `POST /api/{id}/envelope/` (newline-delimited envelope),
//!   * legacy  — `POST /api/{id}/store/`   (a single bare JSON event).
//!
//! Assertions go through the same repository ports the handlers use, so a test
//! proves the whole pipeline end-to-end: auth → decode → parse → fingerprint →
//! persist (issue + event) → response.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use soika::domain::Project;
use soika::ingest::RateLimiter;
use soika::ports::{IssueFilter, NewProject};
use soika::{AppState, Config, MIGRATOR, build_state, router};
use tower::ServiceExt; // for `oneshot`

/// The DSN public key every seeded project is reachable by.
const DSN_KEY: &str = "test-public-key";

/// A running application under test: the wired router plus the seeded project
/// and the state (so tests can assert persisted rows via the ports).
struct TestApp {
    router: Router,
    state: AppState,
    project: Project,
}

impl TestApp {
    /// Build a fresh in-memory app with one seeded project and the default
    /// (production) rate-limit budget.
    async fn spawn() -> TestApp {
        Self::spawn_with_rate_limit(None).await
    }

    /// Like [`spawn`], but overrides the per-project rate-limit budget so the
    /// `429` path is reachable in a handful of requests. `None` keeps the
    /// default budget wired by `build_state`.
    async fn spawn_with_rate_limit(limit: Option<u32>) -> TestApp {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        MIGRATOR.run(&pool).await.expect("run migrations");

        let mut state = build_state(pool, test_config());
        if let Some(limit) = limit {
            // A short window keeps the test fast while still exercising the real
            // fixed-window counter the handler consults.
            state.rate_limiter = Arc::new(RateLimiter::new(limit, Duration::from_secs(60)));
        }

        let team = state
            .teams
            .create("test-team".into())
            .await
            .expect("create team");
        let project = state
            .projects
            .create(NewProject {
                team_id: team.id,
                name: "Test Project".into(),
                slug: "test-project".into(),
                dsn_public_key: DSN_KEY.into(),
                retention_events: 1000,
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
        }
    }

    /// The `/envelope/` URI for the seeded project.
    fn envelope_uri(&self) -> String {
        format!("/api/{}/envelope/", self.project.id)
    }

    /// The legacy `/store/` URI for the seeded project.
    fn store_uri(&self) -> String {
        format!("/api/{}/store/", self.project.id)
    }

    /// POST `body` to `uri` with the given headers; returns the status and the
    /// parsed JSON response (or `Value::Null` for an empty body).
    async fn post(
        &self,
        uri: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> (StatusCode, Value) {
        let (status, _retry_after, json) = self.send(uri, headers, body).await;
        (status, json)
    }

    /// Like [`post`], but also returns the `Retry-After` response header (used by
    /// the rate-limit test).
    async fn send(
        &self,
        uri: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut builder = Request::builder().method("POST").uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(Body::from(body)).expect("build request");

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
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, retry_after, json)
    }

    /// Events currently persisted for the seeded project.
    async fn event_count(&self) -> i64 {
        self.state
            .events
            .count_for_project(self.project.id)
            .await
            .expect("count events")
    }

    /// Issues currently persisted for the seeded project.
    async fn issue_count(&self) -> usize {
        self.state
            .issues
            .list(self.project.id, IssueFilter::default())
            .await
            .expect("list issues")
            .len()
    }
}

/// A minimal config: in-memory db, no SMTP (NoopMailer), signup disabled.
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
    }
}

/// The `X-Sentry-Auth` header value carrying a DSN public key, as real SDKs send.
fn sentry_auth(key: &str) -> String {
    format!("Sentry sentry_version=7, sentry_key={key}, sentry_client=test/1.0")
}

/// Build a modern envelope body: header line, one `event` item header, payload.
fn envelope_body(event_id: &str, payload: &Value) -> Vec<u8> {
    let header = json!({ "event_id": event_id });
    let item_header = json!({ "type": "event" });
    let payload = serde_json::to_string(payload).expect("serialize payload");
    format!("{header}\n{item_header}\n{payload}\n").into_bytes()
}

/// A representative error event payload shared by the wire-format tests.
fn sample_event(event_id: &str) -> Value {
    json!({
        "event_id": event_id,
        "level": "error",
        "exception": {
            "values": [{
                "type": "ValueError",
                "value": "boom",
            }]
        }
    })
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

// --- Modern envelope endpoint ---------------------------------------------

#[tokio::test]
async fn envelope_accepts_event_and_persists() {
    let app = TestApp::spawn().await;
    let event_id = "aaaaaaaaaaaa4aaaaaaaaaaaaaaaaaaa";
    let body = envelope_body(event_id, &sample_event(event_id));

    let (status, json) = app
        .post(
            &app.envelope_uri(),
            &[("x-sentry-auth", &sentry_auth(DSN_KEY))],
            body,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["id"], event_id);
    assert_eq!(app.event_count().await, 1);
    assert_eq!(app.issue_count().await, 1);
}

#[tokio::test]
async fn envelope_unknown_dsn_is_unauthorized() {
    let app = TestApp::spawn().await;
    let event_id = "bbbbbbbbbbbb4bbbbbbbbbbbbbbbbbbb";
    let body = envelope_body(event_id, &sample_event(event_id));

    let (status, _) = app
        .post(
            &app.envelope_uri(),
            &[("x-sentry-auth", &sentry_auth("wrong-key"))],
            body,
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.event_count().await, 0);
}

// --- Legacy store endpoint -------------------------------------------------

#[tokio::test]
async fn store_accepts_legacy_event_and_persists() {
    let app = TestApp::spawn().await;
    let event_id = "cccccccccccc4ccccccccccccccccccc";
    let body = serde_json::to_vec(&sample_event(event_id)).unwrap();

    let (status, json) = app
        .post(
            &app.store_uri(),
            &[("x-sentry-auth", &sentry_auth(DSN_KEY))],
            body,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["id"], event_id);
    assert_eq!(app.event_count().await, 1);
    assert_eq!(app.issue_count().await, 1);
}

#[tokio::test]
async fn store_accepts_key_via_query_param() {
    let app = TestApp::spawn().await;
    let event_id = "dddddddddddd4dddddddddddddddddddd";
    let body = serde_json::to_vec(&sample_event(event_id)).unwrap();
    let uri = format!("{}?sentry_key={DSN_KEY}", app.store_uri());

    let (status, json) = app.post(&uri, &[], body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["id"], event_id);
    assert_eq!(app.event_count().await, 1);
}

#[tokio::test]
async fn store_accepts_gzip_compressed_body() {
    let app = TestApp::spawn().await;
    let event_id = "eeeeeeeeeeee4eeeeeeeeeeeeeeeeeeee";
    let body = gzip(&serde_json::to_vec(&sample_event(event_id)).unwrap());

    let (status, json) = app
        .post(
            &app.store_uri(),
            &[
                ("x-sentry-auth", &sentry_auth(DSN_KEY)),
                ("content-encoding", "gzip"),
            ],
            body,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["id"], event_id);
    assert_eq!(app.event_count().await, 1);
}

#[tokio::test]
async fn store_missing_sentry_key_is_unauthorized() {
    let app = TestApp::spawn().await;
    let event_id = "ffffffffffff4fffffffffffffffffff";
    let body = serde_json::to_vec(&sample_event(event_id)).unwrap();

    let (status, _) = app.post(&app.store_uri(), &[], body).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.event_count().await, 0);
}

#[tokio::test]
async fn store_malformed_json_is_bad_request() {
    let app = TestApp::spawn().await;

    let (status, _) = app
        .post(
            &app.store_uri(),
            &[("x-sentry-auth", &sentry_auth(DSN_KEY))],
            b"not json at all".to_vec(),
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(app.event_count().await, 0);
}

// --- Parity between the two wire formats -----------------------------------

#[tokio::test]
async fn envelope_and_store_group_into_the_same_issue() {
    let app = TestApp::spawn().await;

    // Same logical event shape (only the event_id differs) must fingerprint to
    // the same issue regardless of which endpoint received it.
    let via_envelope = "11111111111141111111111111111111";
    let via_store = "22222222222242222222222222222222";

    let (s1, _) = app
        .post(
            &app.envelope_uri(),
            &[("x-sentry-auth", &sentry_auth(DSN_KEY))],
            envelope_body(via_envelope, &sample_event(via_envelope)),
        )
        .await;
    let (s2, _) = app
        .post(
            &app.store_uri(),
            &[("x-sentry-auth", &sentry_auth(DSN_KEY))],
            serde_json::to_vec(&sample_event(via_store)).unwrap(),
        )
        .await;

    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
    // Two events, one shared issue — proves modern and legacy share grouping.
    assert_eq!(app.event_count().await, 2);
    assert_eq!(app.issue_count().await, 1);
}

// --- Rate limiting ---------------------------------------------------------

#[tokio::test]
async fn over_budget_requests_are_rate_limited_with_retry_after() {
    // Budget of 2 accepted events per window: the 3rd request trips the limit.
    let app = TestApp::spawn_with_rate_limit(Some(2)).await;
    let auth = sentry_auth(DSN_KEY);

    // First two are accepted and persisted.
    for i in 0..2 {
        let event_id = format!("a000000000004000000000000000000{i}");
        let (status, _) = app
            .post(
                &app.store_uri(),
                &[("x-sentry-auth", &auth)],
                serde_json::to_vec(&sample_event(&event_id)).unwrap(),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "request {i} should be accepted");
    }

    // The third is rejected with 429 + a non-zero Retry-After, before any
    // parsing/persistence happens.
    let (status, retry_after, json) = app
        .send(
            &app.store_uri(),
            &[("x-sentry-auth", &auth)],
            serde_json::to_vec(&sample_event("b00000000000400000000000000000bb")).unwrap(),
        )
        .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json["detail"], "rate limited");
    let retry_after: u64 = retry_after
        .expect("retry-after header present")
        .parse()
        .expect("retry-after is numeric");
    assert!(retry_after >= 1, "retry-after should be non-zero");

    // Only the two in-budget events were stored.
    assert_eq!(app.event_count().await, 2);
}

#[tokio::test]
async fn rate_limit_is_per_project() {
    // One project is saturated, a second (different DSN) still has full budget.
    let app = TestApp::spawn_with_rate_limit(Some(1)).await;
    let auth = sentry_auth(DSN_KEY);

    // Saturate the seeded project.
    let (first, _) = app
        .post(
            &app.store_uri(),
            &[("x-sentry-auth", &auth)],
            serde_json::to_vec(&sample_event("c00000000000400000000000000000c1")).unwrap(),
        )
        .await;
    assert_eq!(first, StatusCode::OK);
    let (second, _) = app
        .post(
            &app.store_uri(),
            &[("x-sentry-auth", &auth)],
            serde_json::to_vec(&sample_event("c00000000000400000000000000000c2")).unwrap(),
        )
        .await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);

    // A second project on the same instance shares the limiter but is keyed
    // independently, so its first request is still accepted.
    let other = app
        .state
        .projects
        .create(NewProject {
            team_id: app.project.team_id,
            name: "Other".into(),
            slug: "other-project".into(),
            dsn_public_key: "other-public-key".into(),
            retention_events: 1000,
            retention_days: 0,
            webhook_url: None,
        })
        .await
        .expect("create second project");

    let (status, _) = app
        .post(
            &format!("/api/{}/store/", other.id),
            &[("x-sentry-auth", &sentry_auth("other-public-key"))],
            serde_json::to_vec(&sample_event("d00000000000400000000000000000dd")).unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}
