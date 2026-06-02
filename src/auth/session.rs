//! Server-side session lifecycle & signed session cookie.
//!
//! No JWT: the cookie carries only an opaque, signed session id. The session
//! record (user, expiry) lives in the DB via [`SessionRepository`]. The cookie
//! is `HttpOnly`, `SameSite=Lax`, `Path=/`, and `Secure` when the public
//! `BASE_URL` is HTTPS (so it works over plain HTTP in dev).

use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue};
use chrono::Duration;
use rand::RngCore;
use rand::rngs::OsRng;

use crate::config::Config;
use crate::domain::{Id, Session};
use crate::error::{Error, Result};
use crate::ports::{Clock, SessionRepository};

use super::signing;

/// Name of the session cookie.
pub const SESSION_COOKIE: &str = "soika_session";

/// Default session lifetime: 30 days.
pub const SESSION_TTL_DAYS: i64 = 30;

/// Generate a fresh opaque session id (256 bits of entropy, URL-safe base64).
pub fn generate_session_id() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Build the signed `Set-Cookie` value that establishes a session.
///
/// `secure` controls whether the `Secure` attribute is emitted (true behind TLS).
pub fn build_session_cookie(secret_key: &str, session_id: &str, secure: bool) -> String {
    let signed = signing::sign_str(secret_key.as_bytes(), session_id);
    let max_age = SESSION_TTL_DAYS * 24 * 60 * 60;
    let mut cookie =
        format!("{SESSION_COOKIE}={signed}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build the `Set-Cookie` value that clears the session cookie (logout).
pub fn build_clearing_cookie(secure: bool) -> String {
    let mut cookie = format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Whether session cookies should carry the `Secure` attribute, derived from
/// the public base URL (HTTPS ⇒ secure).
pub fn cookie_secure(config: &Config) -> bool {
    config
        .base_url
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("https://")
}

/// Extract and verify the session id from the request's `Cookie` header.
///
/// Returns `None` when the cookie is absent or its signature is invalid.
pub fn session_id_from_headers(secret_key: &str, headers: &HeaderMap) -> Option<String> {
    let raw = read_cookie(headers, SESSION_COOKIE)?;
    signing::verify_str(secret_key.as_bytes(), &raw).ok()
}

/// Create a new server-side session for `user_id` and return the session record
/// plus the `Set-Cookie` header value to send to the client.
pub async fn start_session(
    sessions: &dyn SessionRepository,
    clock: &dyn Clock,
    config: &Config,
    user_id: Id,
) -> Result<(Session, String)> {
    let session_id = generate_session_id();
    let expires_at = clock.now() + Duration::days(SESSION_TTL_DAYS);
    let session = sessions
        .create(session_id.clone(), user_id, expires_at)
        .await?;
    let cookie = build_session_cookie(&config.secret_key, &session_id, cookie_secure(config));
    Ok((session, cookie))
}

/// Destroy the session referenced by the request cookie (logout). Idempotent:
/// returns the clearing cookie even when no valid session was present.
pub async fn end_session(
    sessions: &dyn SessionRepository,
    config: &Config,
    headers: &HeaderMap,
) -> Result<String> {
    if let Some(session_id) = session_id_from_headers(&config.secret_key, headers) {
        sessions.delete(&session_id).await?;
    }
    Ok(build_clearing_cookie(cookie_secure(config)))
}

/// Append a `Set-Cookie` header to a response header map, validating the value.
pub fn set_cookie_header(headers: &mut HeaderMap, cookie: &str) -> Result<()> {
    let value = HeaderValue::from_str(cookie)
        .map_err(|e| Error::internal(format!("invalid Set-Cookie value: {e}")))?;
    headers.append(SET_COOKIE, value);
    Ok(())
}

/// Read a single cookie value by name from a `Cookie` request header.
fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(COOKIE)?.to_str().ok()?;
    for part in header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=')
            && k.trim() == name
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_unique_and_url_safe() {
        let a = generate_session_id();
        let b = generate_session_id();
        assert_ne!(a, b);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn signed_cookie_round_trips_through_headers() {
        let secret = "cookie-secret";
        let sid = "abc123";
        let cookie = build_session_cookie(secret, sid, true);
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Secure"));

        // Extract the signed value and feed it back as a Cookie header.
        let signed = cookie
            .strip_prefix(&format!("{SESSION_COOKIE}="))
            .and_then(|s| s.split(';').next())
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE}={signed}")).unwrap(),
        );
        let recovered = session_id_from_headers(secret, &headers).unwrap();
        assert_eq!(recovered, sid);
    }

    #[test]
    fn insecure_cookie_omits_secure_attr() {
        let cookie = build_session_cookie("s", "id", false);
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn tampered_cookie_yields_no_session() {
        let secret = "cookie-secret";
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("soika_session=forged.value"),
        );
        assert!(session_id_from_headers(secret, &headers).is_none());
    }

    #[test]
    fn missing_cookie_yields_no_session() {
        let headers = HeaderMap::new();
        assert!(session_id_from_headers("s", &headers).is_none());
    }

    #[test]
    fn cookie_secure_follows_base_url_scheme() {
        let mut cfg = test_config();
        cfg.base_url = "https://errors.example.com".into();
        assert!(cookie_secure(&cfg));
        cfg.base_url = "http://localhost:8080".into();
        assert!(!cookie_secure(&cfg));
    }

    fn test_config() -> Config {
        Config {
            organization_name: "soika".into(),
            database_url: "sqlite::memory:".into(),
            bind_addr: "127.0.0.1:0".into(),
            base_url: "http://localhost:8080".into(),
            secret_key: "secret".into(),
            allow_signup: false,
            default_events_retention: 1000,
            default_retention_days: 0,
            retention_cron: "0 */15 * * * *".into(),
            smtp: None,
            oidc: None,
        }
    }
}
