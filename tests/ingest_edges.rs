//! Integration tests for Sentry-compatible ingestion *edge cases*.
//!
//! Companion to `tests/ingest.rs` (which covers the happy paths and the main
//! tag-mute behaviour). These exercise three narrower edges, all through the
//! public ingest endpoints driven in-process via [`tower::ServiceExt::oneshot`]:
//!
//!   * a body that *claims* `Content-Encoding: gzip` but isn't → `400`, nothing
//!     persisted (`src/ingest/mod.rs` `decode_body` / `gunzip`).
//!   * the tag-mute lookup **failing open**: when the mute-rule repository errors,
//!     the event is still grouped, stored, and *notified* — never silently muted
//!     (`src/ingest/mod.rs::tag_muted`).
//!   * a **tagless** event short-circuiting the mute-rule lookup entirely: the
//!     repository is never consulted, and the event notifies normally
//!     (`src/ingest/mod.rs::tag_muted` empty-tags guard).
//!
//! The harness mirrors `tests/ingest.rs` exactly (fresh `:memory:` SQLite +
//! `MIGRATOR`, one seeded project reachable by [`DSN_KEY`], DSN-authenticated
//! envelope/store requests, assertions via the repository ports). The only
//! addition is the ability to swap in a failing [`TagMuteRuleRepository`] before
//! the router is built, so the fail-open branch is reachable.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use soika::domain::{Id, Project, TagMuteRule};
use soika::error::{Error, Result as SoikaResult};
use soika::ports::{NewProject, NewTagMuteRule, TagMuteRuleRepository};
use soika::{AppState, Config, MIGRATOR, build_state, router};
use tower::ServiceExt; // for `oneshot`

/// The DSN public key every seeded project is reachable by (matches `ingest.rs`).
const DSN_KEY: &str = "test-public-key";

/// A running application under test: the wired router plus the seeded project
/// and the state (so tests can assert persisted rows via the ports).
struct TestApp {
    router: Router,
    state: AppState,
    project: Project,
}

impl TestApp {
    /// Build a fresh in-memory app with one seeded project that carries a
    /// `webhook_url` (so notification delivery is observable) and no DB override.
    async fn spawn_with_webhook(webhook_url: String) -> TestApp {
        Self::spawn_with(webhook_url, None).await
    }

    /// Like [`spawn_with_webhook`], but swaps `AppState::mute_rules` for the
    /// supplied repository *before* the router is built — used to inject a
    /// repository whose `list_for_project` errors and prove the fail-open path.
    async fn spawn_with_webhook_and_mute_rules(
        webhook_url: String,
        mute_rules: Arc<dyn TagMuteRuleRepository>,
    ) -> TestApp {
        Self::spawn_with(webhook_url, Some(mute_rules)).await
    }

    /// Shared builder: seed one project with a webhook, optionally overriding the
    /// tag-mute repository, then build the router from the final state.
    async fn spawn_with(
        webhook_url: String,
        mute_rules: Option<Arc<dyn TagMuteRuleRepository>>,
    ) -> TestApp {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        MIGRATOR.run(&pool).await.expect("run migrations");

        let mut state = build_state(pool, test_config());

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
                webhook_url: Some(webhook_url),
            })
            .await
            .expect("create project");

        // Swap the mute-rule repository AFTER seeding (seeding doesn't touch it)
        // and BEFORE building the router, so the handler sees the override.
        if let Some(mute_rules) = mute_rules {
            state.mute_rules = mute_rules;
        }

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

    /// POST `body` to `uri` with the given headers; returns the status and the
    /// parsed JSON response (or `Value::Null` for an empty body).
    async fn post(
        &self,
        uri: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> (StatusCode, Value) {
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
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, json)
    }

    /// Events currently persisted for the seeded project.
    async fn event_count(&self) -> i64 {
        self.state
            .events
            .count_for_project(self.project.id)
            .await
            .expect("count events")
    }
}

/// A minimal config: in-memory db, no SMTP (NoopMailer), signup disabled
/// (identical to `tests/ingest.rs`).
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
        lockout: Default::default(),
        timezone: "UTC".to_string(),
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

/// An error event payload carrying a `tags` map (mirrors `tests/ingest.rs`).
fn tagged_event(exception_type: &str, tags: &[(&str, &str)]) -> Value {
    let map: serde_json::Map<String, Value> = tags
        .iter()
        .map(|(k, v)| ((*k).to_string(), json!(v)))
        .collect();
    json!({
        "level": "error",
        "exception": { "values": [{ "type": exception_type, "value": "boom" }] },
        "tags": Value::Object(map),
    })
}

/// A tagless error event payload (no `tags` key at all).
fn tagless_event(exception_type: &str) -> Value {
    json!({
        "level": "error",
        "exception": { "values": [{ "type": exception_type, "value": "boom" }] },
    })
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

/// Spawn a webhook capture server that counts every POST it receives and always
/// replies `200`. Returns its base URL and the shared request counter.
///
/// Notification delivery is awaited inside the ingest handler (the webhook POST
/// completes before the envelope response returns), so the counter is settled by
/// the time a `post(...)` call resolves — no sleeps needed. (Copied from
/// `tests/ingest.rs` so this file is self-contained.)
async fn spawn_counting_webhook() -> (String, Arc<AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let server_count = count.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buf = vec![0u8; 8192];
            let _ = socket.read(&mut buf).await;
            server_count.fetch_add(1, Ordering::SeqCst);
            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.flush().await;
        }
    });

    (format!("http://{addr}"), count)
}

/// A [`TagMuteRuleRepository`] whose `list_for_project` always errors, and that
/// counts how many times it was consulted. Used to prove (a) the ingest path
/// fails *open* when the lookup errors and (b) a tagless event never consults it.
struct FailingMuteRules {
    list_calls: Arc<AtomicUsize>,
}

impl FailingMuteRules {
    // Test factory returning (repo, call-counter), not Self — the `new`-returns-Self lint is moot here.
    #[allow(clippy::new_ret_no_self)]
    fn new() -> (Arc<dyn TagMuteRuleRepository>, Arc<AtomicUsize>) {
        let list_calls = Arc::new(AtomicUsize::new(0));
        let repo = Arc::new(FailingMuteRules {
            list_calls: list_calls.clone(),
        });
        (repo, list_calls)
    }
}

#[async_trait]
impl TagMuteRuleRepository for FailingMuteRules {
    async fn create(&self, _new: NewTagMuteRule) -> SoikaResult<TagMuteRule> {
        Err(Error::internal("create not supported in test"))
    }

    async fn list_for_project(&self, _project_id: Id) -> SoikaResult<Vec<TagMuteRule>> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        Err(Error::internal("mute-rule lookup boom"))
    }

    async fn delete(&self, _project_id: Id, _rule_id: Id) -> SoikaResult<bool> {
        Err(Error::internal("delete not supported in test"))
    }
}

// --- decode_body: corrupt "gzip" body ------------------------------------

#[tokio::test]
async fn gzip_encoded_but_not_gzip_body_is_bad_request_and_persists_nothing() {
    // The request advertises `Content-Encoding: gzip` but the bytes are plain
    // text, so `gunzip` errors; `decode_body` surfaces it as a 400 and the
    // pipeline never runs — no issue, no event.
    let (webhook_url, count) = spawn_counting_webhook().await;
    let app = TestApp::spawn_with_webhook(webhook_url).await;

    let (status, json) = app
        .post(
            &app.envelope_uri(),
            &[
                ("x-sentry-auth", &sentry_auth(DSN_KEY)),
                ("content-encoding", "gzip"),
            ],
            b"this is definitely not gzip-compressed".to_vec(),
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["detail"], "could not decode body");
    assert_eq!(app.event_count().await, 0, "no event persisted");
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "a rejected body must not fire a webhook"
    );
}

#[tokio::test]
async fn gzip_header_with_truncated_gzip_is_bad_request() {
    // A *valid* gzip stream that is then truncated mid-payload also fails to
    // decode — same 400, nothing persisted. Guards the magic-byte-present path of
    // `gunzip` (sniffing alone would have accepted the header).
    let (webhook_url, _count) = spawn_counting_webhook().await;
    let app = TestApp::spawn_with_webhook(webhook_url).await;

    let good = gzip(&serde_json::to_vec(&tagless_event("TruncatedError")).unwrap());
    // Drop the trailing bytes so the stream is incomplete but still starts with
    // the gzip magic header.
    let truncated = good[..good.len() / 2].to_vec();

    let (status, json) = app
        .post(
            &app.envelope_uri(),
            &[
                ("x-sentry-auth", &sentry_auth(DSN_KEY)),
                ("content-encoding", "gzip"),
            ],
            truncated,
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["detail"], "could not decode body");
    assert_eq!(app.event_count().await, 0, "no event persisted");
}

// --- tag_muted: fail-open when the lookup errors -------------------------

#[tokio::test]
async fn tag_mute_lookup_error_fails_open_and_still_notifies() {
    // With a mute-rule repository that errors on `list_for_project`, a *tagged*
    // event must still be grouped, stored, and NOTIFIED — the error fails open
    // (returns "not muted") rather than silently swallowing the alert.
    let (webhook_url, webhook_count) = spawn_counting_webhook().await;
    let (mute_rules, list_calls) = FailingMuteRules::new();
    let app = TestApp::spawn_with_webhook_and_mute_rules(webhook_url, mute_rules).await;

    // The payload carries no `event_id`, so the handler synthesizes one — the
    // returned id is not load-bearing here; the observable outcomes are the
    // persisted event, the consulted lookup, and the fired webhook.
    let header_event_id = "aaaaaaaaaaaa4aaaaaaaaaaaaaaaaaaa";
    let (status, _json) = app
        .post(
            &app.envelope_uri(),
            &[("x-sentry-auth", &sentry_auth(DSN_KEY))],
            envelope_body(
                header_event_id,
                &tagged_event("FailOpenError", &[("environment", "staging")]),
            ),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    // Event is processed (the failing lookup must not block ingestion).
    assert_eq!(app.event_count().await, 1, "event still persisted");
    // The lookup was actually consulted (proving we hit the erroring branch)...
    assert!(
        list_calls.load(Ordering::SeqCst) >= 1,
        "the tagged event must consult the mute-rule repository"
    );
    // ...and because it failed open, the new-issue notification still fired.
    assert_eq!(
        webhook_count.load(Ordering::SeqCst),
        1,
        "a failed mute lookup must not suppress the notification"
    );
}

// --- tag_muted: tagless events short-circuit the lookup ------------------

#[tokio::test]
async fn tagless_event_skips_mute_lookup_and_notifies() {
    // A tagless event can never match a (non-empty) rule, so the mute-rule
    // repository must NOT be consulted at all. We wire the always-erroring repo:
    // if the short-circuit were absent the lookup would error (and, fail-open,
    // still notify), but the call counter proves it was never touched.
    let (webhook_url, webhook_count) = spawn_counting_webhook().await;
    let (mute_rules, list_calls) = FailingMuteRules::new();
    let app = TestApp::spawn_with_webhook_and_mute_rules(webhook_url, mute_rules).await;

    // No `event_id` in the payload → the handler synthesizes one; the returned
    // id is not asserted (see the fail-open test for the same rationale).
    let header_event_id = "bbbbbbbbbbbb4bbbbbbbbbbbbbbbbbbb";
    let (status, _json) = app
        .post(
            &app.envelope_uri(),
            &[("x-sentry-auth", &sentry_auth(DSN_KEY))],
            envelope_body(header_event_id, &tagless_event("TaglessError")),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.event_count().await, 1, "event still persisted");
    // The empty-tags guard returns before any DB call.
    assert_eq!(
        list_calls.load(Ordering::SeqCst),
        0,
        "a tagless event must not consult the mute-rule repository"
    );
    // Processed normally → new-issue notification fires.
    assert_eq!(
        webhook_count.load(Ordering::SeqCst),
        1,
        "a tagless new-issue event notifies normally"
    );
}
