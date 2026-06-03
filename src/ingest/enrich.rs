//! Server-side event enrichment.
//!
//! Browser / edge SDKs cannot know the client's public IP and don't ship a
//! parsed browser/OS context — Sentry's own ingestion derives these from the
//! HTTP request at accept time. We mirror that: take the client IP from the
//! connection (honoring proxy headers, via [`crate::auth::client_ip`]) and parse
//! the `User-Agent` into `contexts.browser` / `contexts.os`, then merge them into
//! the event payload.
//!
//! Enrichment is strictly additive: a value the SDK already provided is never
//! overwritten. The one exception is the Sentry `{{auto}}` sentinel for
//! `user.ip_address`, which explicitly asks the server to fill the real IP.

use std::net::SocketAddr;

use axum::http::HeaderMap;
use serde_json::{Map, Value, json};

use crate::auth::client_ip;

/// Sentinel an SDK sets in `user.ip_address` to request server-side resolution.
const AUTO_IP: &str = "{{auto}}";

/// Request-derived metadata used to enrich an event payload. Extracted once per
/// request, then applied to every event item in the envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestMeta {
    /// The resolved client IP, or `None` when it could not be determined.
    pub client_ip: Option<String>,
    /// The raw `User-Agent` header, if present.
    pub user_agent: Option<String>,
    /// The `Referer` header — the page URL the event was reported from.
    pub referer: Option<String>,
    /// The `Accept-Language` header (the client's language preference).
    pub accept_language: Option<String>,
}

impl RequestMeta {
    /// Pull enrichment inputs from the request headers and TCP peer address.
    pub fn from_request(headers: &HeaderMap, peer: Option<SocketAddr>) -> Self {
        let ip = client_ip(headers, peer);
        RequestMeta {
            // `client_ip` returns the literal "unknown" when it has no source;
            // treat that as absent so we never store a bogus address.
            client_ip: (ip != "unknown").then_some(ip),
            user_agent: header_str(headers, "user-agent"),
            referer: header_str(headers, "referer"),
            accept_language: header_str(headers, "accept-language"),
        }
    }
}

/// Merge request-derived metadata into an event `payload` in place.
///
/// Fills, without clobbering SDK-provided values:
/// - `user.ip_address` (also replacing the `{{auto}}` sentinel);
/// - `contexts.browser` / `contexts.os` parsed from the User-Agent;
/// - `request.url` (from `Referer`), `request.headers` (User-Agent,
///   Referer, Accept-Language), and `request.env.REMOTE_ADDR`.
pub fn enrich(payload: &mut Value, meta: &RequestMeta) {
    // Only objects carry the Sentry event shape; anything else is left alone.
    let Some(obj) = payload.as_object_mut() else {
        return;
    };

    if let Some(ip) = meta.client_ip.as_deref() {
        set_ip_address(obj, ip);
    }

    if let Some(ua) = meta.user_agent.as_deref() {
        set_ua_contexts(obj, ua);
    }

    set_request(obj, meta);
}

/// Set `user.ip_address` when missing, empty, or the `{{auto}}` sentinel.
fn set_ip_address(event: &mut Map<String, Value>, ip: &str) {
    let user = ensure_object(event, "user");
    match user.get("ip_address").and_then(Value::as_str) {
        Some(existing) if existing != AUTO_IP && !existing.trim().is_empty() => {}
        _ => {
            user.insert("ip_address".into(), json!(ip));
        }
    }
}

/// Parse the User-Agent and fill `contexts.browser` / `contexts.os` if absent.
fn set_ua_contexts(event: &mut Map<String, Value>, ua: &str) {
    let parsed = parse_user_agent(ua);
    if parsed.is_empty() {
        return;
    }

    let contexts = ensure_object(event, "contexts");
    if let Some((name, version)) = parsed.browser {
        set_named_context(contexts, "browser", name, version);
    }
    if let Some((name, version)) = parsed.os {
        set_named_context(contexts, "os", name, version);
    }
}

/// Insert a `{ type, name, version }` context entry under `key` when absent.
fn set_named_context(
    contexts: &mut Map<String, Value>,
    key: &str,
    name: String,
    version: Option<String>,
) {
    if contexts.contains_key(key) {
        return;
    }
    let mut entry = Map::new();
    // Sentry context objects carry a `type` discriminator equal to the key.
    entry.insert("type".into(), json!(key));
    entry.insert("name".into(), json!(name));
    if let Some(version) = version {
        entry.insert("version".into(), json!(version));
    }
    contexts.insert(key.into(), Value::Object(entry));
}

/// Fill the `request` interface from the headers: URL (from Referer), a few
/// useful headers, and `env.REMOTE_ADDR`. Existing keys are preserved.
fn set_request(event: &mut Map<String, Value>, meta: &RequestMeta) {
    // Skip entirely when there's nothing to add.
    let nothing = meta.referer.is_none()
        && meta.user_agent.is_none()
        && meta.accept_language.is_none()
        && meta.client_ip.is_none();
    if nothing {
        return;
    }

    let request = ensure_object(event, "request");

    if let Some(referer) = meta.referer.as_deref()
        && !request.contains_key("url")
    {
        request.insert("url".into(), json!(referer));
    }

    let headers = ensure_object(request, "headers");
    insert_header_if_absent(headers, "User-Agent", meta.user_agent.as_deref());
    insert_header_if_absent(headers, "Referer", meta.referer.as_deref());
    insert_header_if_absent(headers, "Accept-Language", meta.accept_language.as_deref());

    if let Some(ip) = meta.client_ip.as_deref() {
        let env = ensure_object(request, "env");
        if !env.contains_key("REMOTE_ADDR") {
            env.insert("REMOTE_ADDR".into(), json!(ip));
        }
    }
}

/// Insert a request header (preserving any the SDK already sent).
fn insert_header_if_absent(headers: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value
        && !headers.contains_key(key)
    {
        headers.insert(key.into(), json!(value));
    }
}

/// Get (or create) a nested JSON object under `key`. If `key` exists but is not
/// an object, it is replaced with a fresh object so enrichment can proceed.
fn ensure_object<'a>(parent: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    if !parent.get(key).map(Value::is_object).unwrap_or(false) {
        parent.insert(key.into(), Value::Object(Map::new()));
    }
    parent
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("just ensured object")
}

/// Read a header as an owned, trimmed, non-empty string.
fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    let value = headers.get(name)?.to_str().ok()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

// --- User-Agent parsing -----------------------------------------------------

/// A `(name, optional version)` pair parsed from a User-Agent.
type Product = (String, Option<String>);

/// The browser/OS products parsed from a User-Agent string.
#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedUa {
    browser: Option<Product>,
    os: Option<Product>,
}

impl ParsedUa {
    fn is_empty(&self) -> bool {
        self.browser.is_none() && self.os.is_none()
    }
}

/// Parse a User-Agent into a best-effort browser + OS.
///
/// Dependency-free and intentionally small: it recognizes the mainstream
/// desktop/mobile browsers and operating systems. Order matters — Edge, Opera,
/// and the Chromium-based Samsung browser all masquerade as Chrome, so they are
/// matched first.
fn parse_user_agent(ua: &str) -> ParsedUa {
    ParsedUa {
        browser: parse_browser(ua),
        os: parse_os(ua),
    }
}

fn parse_browser(ua: &str) -> Option<Product> {
    // Each entry: the browser name and the token its version follows. Probed in
    // order so Chrome-impersonators are caught before the generic Chrome rule.
    const RULES: &[(&str, &str)] = &[
        ("Microsoft Edge", "Edg/"),
        ("Microsoft Edge", "EdgA/"),
        ("Microsoft Edge", "EdgiOS/"),
        ("Microsoft Edge", "Edge/"),
        ("Opera", "OPR/"),
        ("Opera", "Opera/"),
        ("Samsung Internet", "SamsungBrowser/"),
        ("Firefox", "Firefox/"),
        ("Firefox", "FxiOS/"),
        ("Chrome", "Chrome/"),
        ("Chrome", "CriOS/"),
    ];

    for (name, token) in RULES {
        if let Some(version) = version_after(ua, token) {
            return Some((name.to_string(), Some(version)));
        }
    }

    // Safari reports its release in `Version/`, with `Safari/` present too. Only
    // treat it as Safari once the Chromium family above has been ruled out.
    if ua.contains("Safari/") {
        let version = version_after(ua, "Version/");
        return Some(("Safari".to_string(), version));
    }

    // Legacy Internet Explorer.
    if ua.contains("Trident/") || ua.contains("MSIE ") {
        let version = version_after(ua, "rv:").or_else(|| version_after(ua, "MSIE "));
        return Some(("Internet Explorer".to_string(), version));
    }

    None
}

fn parse_os(ua: &str) -> Option<Product> {
    // iOS / iPadOS before macOS (iPad UAs also contain "Mac OS X").
    if ua.contains("iPhone") || ua.contains("iPad") || ua.contains("iPod") {
        let version = version_token(ua, "OS ").map(|v| v.replace('_', "."));
        return Some(("iOS".to_string(), version));
    }
    if ua.contains("Android") {
        return Some(("Android".to_string(), version_token(ua, "Android ")));
    }
    if ua.contains("Windows NT") {
        let version = version_token(ua, "Windows NT ").map(map_windows_nt);
        return Some(("Windows".to_string(), version));
    }
    if ua.contains("Mac OS X") {
        let version = version_token(ua, "Mac OS X ").map(|v| v.replace('_', "."));
        return Some(("macOS".to_string(), version));
    }
    if ua.contains("CrOS") {
        return Some(("Chrome OS".to_string(), None));
    }
    if ua.contains("Linux") {
        return Some(("Linux".to_string(), None));
    }
    None
}

/// Map a `Windows NT` kernel version to its marketing name (e.g. `10.0` → `10`).
fn map_windows_nt(nt: String) -> String {
    match nt.as_str() {
        "10.0" => "10".to_string(),
        "6.3" => "8.1".to_string(),
        "6.2" => "8".to_string(),
        "6.1" => "7".to_string(),
        "6.0" => "Vista".to_string(),
        "5.1" | "5.2" => "XP".to_string(),
        other => other.to_string(),
    }
}

/// The version string immediately following `token` (e.g. after `Chrome/`),
/// terminated by a space, `;`, `)`, or `,`. Empty results yield `None`.
fn version_after(ua: &str, token: &str) -> Option<String> {
    let rest = &ua[ua.find(token)? + token.len()..];
    let end = rest.find([' ', ';', ')', ',']).unwrap_or(rest.len());
    let version = rest[..end].trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// A version token following `token`, restricted to the leading
/// digit/dot/underscore run (e.g. `Windows NT 10.0; Win64` → `10.0`).
fn version_token(ua: &str, token: &str) -> Option<String> {
    let rest = &ua[ua.find(token)? + token.len()..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '_'))
        .unwrap_or(rest.len());
    let version = &rest[..end];
    (!version.is_empty()).then(|| version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    const CHROME_WIN: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
        (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
    const SAFARI_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
        (KHTML, like Gecko) Version/17.1 Safari/605.1.15";
    const FIREFOX_LINUX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 \
        Firefox/121.0";
    const EDGE_WIN: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
        (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0";
    const SAFARI_IOS: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_1 like Mac OS X) \
        AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Mobile/15E148 Safari/604.1";
    const CHROME_ANDROID: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 \
        (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";

    #[test]
    fn parses_chrome_on_windows() {
        let p = parse_user_agent(CHROME_WIN);
        assert_eq!(p.browser, Some(("Chrome".into(), Some("120.0.0.0".into()))));
        assert_eq!(p.os, Some(("Windows".into(), Some("10".into()))));
    }

    #[test]
    fn parses_safari_on_macos() {
        let p = parse_user_agent(SAFARI_MAC);
        assert_eq!(p.browser, Some(("Safari".into(), Some("17.1".into()))));
        assert_eq!(p.os, Some(("macOS".into(), Some("10.15.7".into()))));
    }

    #[test]
    fn parses_firefox_on_linux() {
        let p = parse_user_agent(FIREFOX_LINUX);
        assert_eq!(p.browser, Some(("Firefox".into(), Some("121.0".into()))));
        assert_eq!(p.os, Some(("Linux".into(), None)));
    }

    #[test]
    fn edge_is_not_misreported_as_chrome() {
        let p = parse_user_agent(EDGE_WIN);
        assert_eq!(
            p.browser,
            Some(("Microsoft Edge".into(), Some("120.0.0.0".into())))
        );
    }

    #[test]
    fn parses_safari_on_ios() {
        let p = parse_user_agent(SAFARI_IOS);
        assert_eq!(p.browser, Some(("Safari".into(), Some("17.1".into()))));
        assert_eq!(p.os, Some(("iOS".into(), Some("17.1".into()))));
    }

    #[test]
    fn parses_chrome_on_android() {
        let p = parse_user_agent(CHROME_ANDROID);
        assert_eq!(p.browser, Some(("Chrome".into(), Some("120.0.0.0".into()))));
        assert_eq!(p.os, Some(("Android".into(), Some("13".into()))));
    }

    #[test]
    fn unknown_ua_yields_nothing() {
        assert!(parse_user_agent("curl/8.4.0").browser.is_none());
        assert!(parse_user_agent("totally-bogus").is_empty());
    }

    #[test]
    fn enrich_sets_ip_browser_and_request() {
        let meta = RequestMeta {
            client_ip: Some("203.0.113.7".into()),
            user_agent: Some(CHROME_WIN.into()),
            referer: Some("https://app.example.com/dashboard".into()),
            accept_language: Some("en-US,en;q=0.9".into()),
        };
        let mut payload = json!({ "exception": { "values": [] } });
        enrich(&mut payload, &meta);

        assert_eq!(payload["user"]["ip_address"], json!("203.0.113.7"));
        assert_eq!(payload["contexts"]["browser"]["name"], json!("Chrome"));
        assert_eq!(payload["contexts"]["os"]["name"], json!("Windows"));
        assert_eq!(
            payload["request"]["url"],
            json!("https://app.example.com/dashboard")
        );
        assert_eq!(
            payload["request"]["headers"]["User-Agent"],
            json!(CHROME_WIN)
        );
        assert_eq!(
            payload["request"]["env"]["REMOTE_ADDR"],
            json!("203.0.113.7")
        );
    }

    #[test]
    fn enrich_replaces_auto_ip_sentinel() {
        let meta = RequestMeta {
            client_ip: Some("198.51.100.9".into()),
            ..RequestMeta::default()
        };
        let mut payload = json!({ "user": { "id": "u1", "ip_address": "{{auto}}" } });
        enrich(&mut payload, &meta);
        assert_eq!(payload["user"]["ip_address"], json!("198.51.100.9"));
        // Unrelated user fields are preserved.
        assert_eq!(payload["user"]["id"], json!("u1"));
    }

    #[test]
    fn enrich_preserves_sdk_supplied_values() {
        let meta = RequestMeta {
            client_ip: Some("203.0.113.7".into()),
            user_agent: Some(CHROME_WIN.into()),
            referer: Some("https://server.example/ref".into()),
            ..RequestMeta::default()
        };
        let mut payload = json!({
            "user": { "ip_address": "10.0.0.1" },
            "contexts": { "browser": { "name": "CustomBrowser", "version": "9" } },
            "request": { "url": "https://real.example/page" }
        });
        enrich(&mut payload, &meta);

        // SDK-provided IP, browser, and URL all win over server-derived values.
        assert_eq!(payload["user"]["ip_address"], json!("10.0.0.1"));
        assert_eq!(
            payload["contexts"]["browser"]["name"],
            json!("CustomBrowser")
        );
        assert_eq!(
            payload["request"]["url"],
            json!("https://real.example/page")
        );
        // OS, which the SDK omitted, is still filled in.
        assert_eq!(payload["contexts"]["os"]["name"], json!("Windows"));
    }

    #[test]
    fn enrich_ignores_non_object_payload() {
        let mut payload = json!("not an object");
        enrich(&mut payload, &RequestMeta::default());
        assert_eq!(payload, json!("not an object"));
    }

    #[test]
    fn request_meta_from_headers_resolves_ip_and_ua() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));
        headers.insert("user-agent", HeaderValue::from_static(CHROME_WIN));
        headers.insert("referer", HeaderValue::from_static("https://app.example/x"));

        let meta = RequestMeta::from_request(&headers, None);
        assert_eq!(meta.client_ip.as_deref(), Some("203.0.113.7"));
        assert_eq!(meta.user_agent.as_deref(), Some(CHROME_WIN));
        assert_eq!(meta.referer.as_deref(), Some("https://app.example/x"));
    }

    #[test]
    fn request_meta_unknown_ip_becomes_none() {
        let meta = RequestMeta::from_request(&HeaderMap::new(), None);
        assert_eq!(meta.client_ip, None);
    }
}
