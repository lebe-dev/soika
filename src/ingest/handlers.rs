//! Sentry-compatible ingestion HTTP handlers (envelope + legacy store).
//!
//! Pipeline: auth → decode → parse → normalize → fingerprint → upsert issue →
//! insert event → counters → notify.

use std::io::Read;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use uuid::Uuid;

use super::enrich::{RequestMeta, enrich};
use super::envelope::{self, EnvelopeItem};
use super::ratelimit::Decision;
use super::tags;
use crate::auth::PeerAddr;
use crate::domain::stacktrace::NormalizedEvent;
use crate::domain::{Project, Timestamp};
use crate::grouping::{self, IngestInput, NotifyKind};
use crate::notify;
use crate::state::AppState;

/// `POST /api/{project_id}/envelope/` — Sentry envelope ingestion.
///
/// Returns `{ "id": "<event_id>" }` on accept; honors `429` rate-limit
/// semantics.
pub async fn envelope(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    RawQuery(query): RawQuery,
    PeerAddr(peer): PeerAddr,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Shared preamble: DSN auth → project resolve → rate-limit → decode.
    let (project, decoded) =
        match authorize_and_decode(&state, &project_id, query.as_deref(), &headers, &body).await {
            Ok(ok) => ok,
            Err(response) => return response,
        };

    // --- Parse the Sentry envelope. ---
    let parsed = match envelope::parse(&decoded) {
        Ok(env) => env,
        Err(e) => {
            tracing::debug!(error = %e, project_id = %project.id, "ingest dropped malformed envelope");
            return bad_request("malformed envelope");
        }
    };

    // Request-derived enrichment (client IP, browser/OS) applied to every event.
    let meta = RequestMeta::from_request(&headers, peer);
    let header_event_id = parsed.header_event_id();
    let now = state.clock.now();

    // Process every event item; non-event items are acked + discarded.
    let mut accepted_event_id: Option<String> = None;
    for item in &parsed.items {
        if !is_ingestible_event(item) {
            // transactions / sessions / attachments / etc. → parse, ack, discard.
            continue;
        }

        let payload = match item.payload_json() {
            Ok(value) => value,
            // A malformed event payload is dropped (acked) so the SDK won't retry.
            Err(e) => {
                tracing::debug!(error = %e, project_id = %project.id, "ingest dropped malformed event payload");
                continue;
            }
        };

        match process_event(&state, &project, payload, &meta, now).await {
            Ok(event_id) => {
                if accepted_event_id.is_none() {
                    accepted_event_id = Some(event_id);
                }
            }
            Err(e) => {
                tracing::error!(error = %e, project_id = %project.id, "ingest failed to process event");
                return internal_error();
            }
        }
    }

    // Sentry-style success response: echo an event id. Fall back to the
    // envelope header id, or synthesize one, so SDKs always receive an id.
    let id = accepted_event_id
        .or(header_event_id)
        .unwrap_or_else(new_event_id);

    (StatusCode::OK, Json(json!({ "id": id }))).into_response()
}

/// `POST /api/{project_id}/store/` — legacy Sentry store endpoint.
///
/// Unlike [`envelope`], the body is a single JSON event payload (the classic
/// pre-envelope wire format), optionally gzip/zlib-compressed. Auth,
/// rate-limiting, decoding, and the grouping pipeline are shared; only the wire
/// format differs. Returns `{ "id": "<event_id>" }` on accept.
pub async fn store(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    RawQuery(query): RawQuery,
    PeerAddr(peer): PeerAddr,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (project, decoded) =
        match authorize_and_decode(&state, &project_id, query.as_deref(), &headers, &body).await {
            Ok(ok) => ok,
            Err(response) => return response,
        };

    // Legacy bodies are a bare event object, not a newline-delimited envelope.
    let payload: Value = match serde_json::from_slice(&decoded) {
        Ok(value) => value,
        Err(e) => {
            tracing::debug!(error = %e, project_id = %project.id, "ingest dropped malformed event payload");
            return bad_request("malformed event payload");
        }
    };

    let meta = RequestMeta::from_request(&headers, peer);
    let now = state.clock.now();
    match process_event(&state, &project, payload, &meta, now).await {
        // `process_event` derives the id from the payload's `event_id` (or
        // synthesizes one), matching the envelope path's behavior.
        Ok(event_id) => (StatusCode::OK, Json(json!({ "id": event_id }))).into_response(),
        Err(e) => {
            tracing::error!(error = %e, project_id = %project.id, "ingest failed to process event");
            internal_error()
        }
    }
}

/// Shared ingestion preamble for the `envelope` and `store` endpoints:
/// resolve the project from the DSN public key, apply the soft per-project rate
/// limit, then decode the (possibly compressed) body.
///
/// On success returns the resolved project and the decoded body. On failure
/// returns the Sentry-compatible early [`Response`] the handler should send.
async fn authorize_and_decode(
    state: &AppState,
    project_id: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: &Bytes,
) -> std::result::Result<(Project, Vec<u8>), Response> {
    // --- Auth: resolve project from the DSN public key. ---
    let Some(dsn_key) = extract_dsn_key(headers, query) else {
        tracing::debug!("ingest rejected: missing sentry key");
        return Err(unauthorized("missing sentry key"));
    };

    // Never log the raw DSN key.
    let project = match state.projects.find_by_dsn(&dsn_key).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::debug!("ingest rejected: unknown sentry key");
            return Err(unauthorized("unknown sentry key"));
        }
        Err(e) => {
            tracing::error!(error = %e, "ingest failed to resolve project from DSN");
            return Err(internal_error());
        }
    };

    // The path `{project_id}` is advisory; the DSN is authoritative. If the SDK
    // sends a numeric/uuid project id that disagrees with the resolved project,
    // we still accept (Sentry routes purely by key) but note the mismatch.
    if !project_id.is_empty() && project_id != project.id.to_string() {
        tracing::debug!(
            path_project_id = %project_id,
            resolved_project_id = %project.id,
            "ingest path project id differs from DSN-resolved project"
        );
    }

    // --- Soft per-project rate limit. ---
    let project_key = project.id.to_string();
    if let Decision::Limited { retry_after_secs } = state.rate_limiter.check(&project_key) {
        tracing::debug!(project_id = %project.id, "ingest rate-limited");
        return Err(rate_limited(retry_after_secs));
    }

    // --- Decode the (possibly compressed) body. ---
    match decode_body(headers, body) {
        Ok(decoded) => Ok((project, decoded)),
        Err(e) => {
            tracing::debug!(error = %e, project_id = %project.id, "ingest could not decode body");
            Err(bad_request("could not decode body"))
        }
    }
}

/// Persist a single parsed event through the grouping pipeline:
/// fingerprint → upsert issue → insert event → notify. Returns the stored
/// event id.
///
/// The grouping/persistence tail is the unit-tested [`grouping::ingest_normalized`]
/// (hexagonal core); this handler only parses the payload once, runs that
/// pipeline, then dispatches the notification it asks for.
async fn process_event(
    state: &AppState,
    project: &Project,
    mut payload: Value,
    meta: &RequestMeta,
    now: Timestamp,
) -> crate::error::Result<String> {
    // Fill request-derived context (client IP, browser/OS) the SDK can't send,
    // before the payload is normalized for grouping and persisted.
    enrich(&mut payload, meta);

    let event_id = event_id_from_payload(&payload).unwrap_or_else(new_event_id);
    let normalized = NormalizedEvent::from_value(&payload);

    // Tag-mute is evaluated against the event's tags (read before `payload` is
    // moved into the pipeline input). It only suppresses notifications — the
    // event is still grouped, stored, and counted below.
    let event_tags = tags::event_tags(&payload);

    let outcome = grouping::ingest_normalized(
        state.issues.as_ref(),
        state.events.as_ref(),
        IngestInput {
            project_id: project.id,
            event_id: event_id.clone(),
            normalized,
            payload,
        },
        now,
    )
    .await?;

    // Notifications respect project-level mute and tag-mute rules. Issue-level
    // mute and per-user opt-out are enforced inside the notify module.
    if !project.muted && !tag_muted(state, project.id, &event_tags).await {
        notify_outcome(state, project, &outcome.issue, outcome.notify).await;
    }

    Ok(event_id)
}

/// True if any of the project's tag-mute rules matches this event's tags.
///
/// Fails open: a repository error logs and returns `false` (notify) rather than
/// silently swallowing an alert. Skips the DB call entirely when the event has
/// no tags, since an empty-tag event can never match a (non-empty) rule.
async fn tag_muted(
    state: &AppState,
    project_id: crate::domain::Id,
    event_tags: &std::collections::BTreeMap<String, String>,
) -> bool {
    if event_tags.is_empty() {
        return false;
    }
    match state.mute_rules.list_for_project(project_id).await {
        Ok(rules) => rules.iter().any(|rule| rule.matches(event_tags)),
        Err(err) => {
            tracing::warn!(error = %err, %project_id, "tag-mute lookup failed; not muting");
            false
        }
    }
}

/// Fire the notification the pipeline asked for. Notification failures must
/// never fail ingestion, so errors are swallowed (logged).
async fn notify_outcome(
    state: &AppState,
    project: &Project,
    issue: &crate::domain::Issue,
    notify: NotifyKind,
) {
    let result = match notify {
        NotifyKind::NewIssue => notify::notify_new_issue(state, project, issue).await,
        NotifyKind::Regression => notify::notify_regression(state, project, issue).await,
        NotifyKind::None => return,
    };

    if let Err(err) = result {
        tracing::warn!(error = %err, issue_id = %issue.id, "notification failed");
    }
}

/// True if an envelope item is an event we ingest: `type=event` whose payload is
/// an error/exception or message. Item-type screening only; payload
/// shape is validated when parsed.
fn is_ingestible_event(item: &EnvelopeItem) -> bool {
    matches!(item.item_type(), Some("event"))
}

/// Extract the DSN public key from `X-Sentry-Auth` (`sentry_key=…`), the
/// `?sentry_key=` query parameter, or the `X-Sentry-Key` header. The
/// header takes precedence, then the query, then the bare key header.
fn extract_dsn_key(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(value) = headers
        .get("x-sentry-auth")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_sentry_auth_key)
    {
        return Some(value);
    }

    if let Some(value) = query.and_then(sentry_key_from_query) {
        return Some(value);
    }

    headers
        .get("x-sentry-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Pull `sentry_key` out of a raw query string (`a=b&sentry_key=abc&c=d`).
fn sentry_key_from_query(query: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == "sentry_key")
        .map(|(_, v)| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// Parse the `sentry_key` token out of an `X-Sentry-Auth` header value such as
/// `Sentry sentry_version=7, sentry_key=abc123, sentry_client=...`.
///
/// The header is a `Sentry`-scheme credential whose tokens are separated by
/// commas and/or whitespace; the leading `Sentry` scheme word carries no `=`
/// and is skipped. Key matching is case-insensitive and values may be quoted.
fn parse_sentry_auth_key(value: &str) -> Option<String> {
    value
        .split([',', ' ', '\t'])
        .filter_map(|part| part.trim().split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("sentry_key"))
        .map(|(_, v)| v.trim().trim_matches('"').to_owned())
        .filter(|v| !v.is_empty())
}

/// Decode the request body, honoring `Content-Encoding` and sniffing magic bytes
/// for gzip/zlib; uncompressed bodies pass through unchanged.
fn decode_body(headers: &HeaderMap, body: &[u8]) -> std::io::Result<Vec<u8>> {
    let encoding = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase());

    match encoding.as_deref() {
        Some("gzip") | Some("x-gzip") => gunzip(body),
        Some("deflate") | Some("zlib") => inflate_zlib(body),
        Some("identity") | None => decode_sniffed(body),
        // Unknown encoding: best-effort sniff so we don't reject valid bodies.
        Some(_) => decode_sniffed(body),
    }
}

/// When no explicit encoding is set, sniff the leading magic bytes.
fn decode_sniffed(body: &[u8]) -> std::io::Result<Vec<u8>> {
    if body.len() >= 2 && body[0] == 0x1f && body[1] == 0x8b {
        return gunzip(body);
    }
    // zlib header: CMF=0x78 with a valid FCHECK; accept the common variants.
    if body.len() >= 2 && body[0] == 0x78 && matches!(body[1], 0x01 | 0x9c | 0xda | 0x5e) {
        return inflate_zlib(body);
    }
    Ok(body.to_vec())
}

fn gunzip(body: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(body);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn inflate_zlib(body: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(body);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// Read a normalized 32-char hex `event_id` from an event payload.
fn event_id_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("event_id")
        .and_then(Value::as_str)
        .map(normalize_event_id)
        .filter(|s| !s.is_empty())
}

/// Sentry `event_id` is hex32 without dashes; strip dashes the SDK may include.
fn normalize_event_id(raw: &str) -> String {
    raw.trim().replace('-', "").to_ascii_lowercase()
}

/// Generate a fresh hex32 event id (no dashes), matching Sentry's format.
fn new_event_id() -> String {
    Uuid::new_v4().simple().to_string()
}

// --- Response helpers (Sentry-compatible status codes). ---

fn unauthorized(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "detail": msg }))).into_response()
}

fn bad_request(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "detail": msg }))).into_response()
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "detail": "internal error" })),
    )
        .into_response()
}

fn rate_limited(retry_after_secs: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", retry_after_secs.to_string())],
        Json(json!({ "detail": "rate limited" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn parses_sentry_key_from_auth_header() {
        let v = "Sentry sentry_version=7, sentry_key=abc123def, sentry_client=sentry.js/7.0";
        assert_eq!(parse_sentry_auth_key(v).as_deref(), Some("abc123def"));
    }

    #[test]
    fn parses_sentry_key_case_insensitive_and_quoted() {
        let v = "Sentry Sentry_Key=\"deadbeef\"";
        assert_eq!(parse_sentry_auth_key(v).as_deref(), Some("deadbeef"));
    }

    #[test]
    fn missing_sentry_key_returns_none() {
        let v = "Sentry sentry_version=7, sentry_client=x";
        assert_eq!(parse_sentry_auth_key(v), None);
    }

    #[test]
    fn extract_dsn_key_prefers_auth_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-sentry-auth",
            HeaderValue::from_static("Sentry sentry_key=fromauth"),
        );
        headers.insert("x-sentry-key", HeaderValue::from_static("fromkey"));
        assert_eq!(
            extract_dsn_key(&headers, Some("sentry_key=fromquery")).as_deref(),
            Some("fromauth")
        );
    }

    #[test]
    fn extract_dsn_key_uses_query_when_no_auth_header() {
        let headers = HeaderMap::new();
        assert_eq!(
            extract_dsn_key(&headers, Some("foo=bar&sentry_key=fromquery&x=1")).as_deref(),
            Some("fromquery")
        );
    }

    #[test]
    fn extract_dsn_key_falls_back_to_key_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-sentry-key", HeaderValue::from_static("fromkey"));
        assert_eq!(extract_dsn_key(&headers, None).as_deref(), Some("fromkey"));
    }

    #[test]
    fn extract_dsn_key_absent_returns_none() {
        let headers = HeaderMap::new();
        assert_eq!(extract_dsn_key(&headers, None), None);
    }

    #[test]
    fn sentry_key_from_query_parses_param() {
        assert_eq!(
            sentry_key_from_query("a=1&sentry_key=abc123&b=2").as_deref(),
            Some("abc123")
        );
        assert_eq!(sentry_key_from_query("a=1&b=2"), None);
        assert_eq!(sentry_key_from_query("sentry_key="), None);
    }

    #[test]
    fn decodes_plain_body() {
        let headers = HeaderMap::new();
        let out = decode_body(&headers, b"hello").expect("decode");
        assert_eq!(out, b"hello");
    }

    #[test]
    fn decodes_gzip_body_via_content_encoding() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"compressed payload").unwrap();
        let gz = enc.finish().unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("content-encoding", HeaderValue::from_static("gzip"));
        let out = decode_body(&headers, &gz).expect("decode");
        assert_eq!(out, b"compressed payload");
    }

    #[test]
    fn decodes_gzip_body_via_sniffing() {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"sniffed gzip").unwrap();
        let gz = enc.finish().unwrap();

        // No content-encoding header → sniff magic bytes.
        let headers = HeaderMap::new();
        let out = decode_body(&headers, &gz).expect("decode");
        assert_eq!(out, b"sniffed gzip");
    }

    #[test]
    fn decodes_zlib_body() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"zlib payload").unwrap();
        let z = enc.finish().unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("content-encoding", HeaderValue::from_static("deflate"));
        let out = decode_body(&headers, &z).expect("decode");
        assert_eq!(out, b"zlib payload");
    }

    #[test]
    fn normalizes_event_id() {
        assert_eq!(
            normalize_event_id("9EC79C33-EC99-42AB-8353-589FCB2E04C0"),
            "9ec79c33ec9942ab8353589fcb2e04c0"
        );
    }

    #[test]
    fn event_id_from_payload_reads_field() {
        let payload = json!({ "event_id": "fc6d8c0c43fc4630ad850ee518f1b9d0" });
        assert_eq!(
            event_id_from_payload(&payload).as_deref(),
            Some("fc6d8c0c43fc4630ad850ee518f1b9d0")
        );
    }

    #[test]
    fn event_id_from_payload_absent_is_none() {
        assert_eq!(event_id_from_payload(&json!({})), None);
    }

    #[test]
    fn new_event_id_is_hex32_no_dashes() {
        let id = new_event_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn ingestible_only_for_event_type() {
        let event = EnvelopeItem {
            header: json!({ "type": "event" }),
            payload: b"{}".to_vec(),
        };
        let session = EnvelopeItem {
            header: json!({ "type": "session" }),
            payload: b"{}".to_vec(),
        };
        let transaction = EnvelopeItem {
            header: json!({ "type": "transaction" }),
            payload: b"{}".to_vec(),
        };
        assert!(is_ingestible_event(&event));
        assert!(!is_ingestible_event(&session));
        assert!(!is_ingestible_event(&transaction));
    }
}
