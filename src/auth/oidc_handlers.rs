//! OIDC HTTP handlers: the browser-facing `/auth/oidc/*` routes plus the public
//! `/auth/config` endpoint.
//!
//! These are deliberately thin: the flow logic and ID-token validation live in
//! [`crate::auth::oidc`]. Handlers translate between HTTP (cookies, redirects,
//! query params) and that flow, provisioning a user on first SSO login.
//!
//! Unlike the JSON auth handlers, the login/callback routes are *browser
//! navigations*: on success or failure they emit `302` redirects (to the
//! provider, to `next`/`/`, or to `/login?error=...`) rather than JSON bodies
//!.

use axum::extract::{Query, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use openidconnect::{Nonce, PkceCodeVerifier};
use serde::{Deserialize, Serialize};

use crate::api::client_config::ClientConfigView;
use crate::api::profile::ProfileView;
use crate::api::teams::OptionalAuthUser;
use crate::domain::AuthProvider;
use crate::error::Error;
use crate::ports::NewUser;
use crate::state::AppState;

use super::oidc::{
    STATE_COOKIE, build_clearing_state_cookie, build_state_cookie, decode_state_cookie,
    email_domain_allowed, encode_state_cookie,
};
use super::session::{cookie_secure, start_session};

// ---------------------------------------------------------------------------
// Query types
// ---------------------------------------------------------------------------

/// `?next=/path` on `/auth/oidc/login`.
#[derive(Debug, Default, Deserialize)]
pub struct LoginQuery {
    #[serde(default)]
    pub next: Option<String>,
}

/// `?code&state` returned by the provider on `/auth/oidc/callback`.
#[derive(Debug, Default, Deserialize)]
pub struct CallbackQuery {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

/// Bootstrap configuration for the SPA, served on every page load.
///
/// The first block (`oauth_*`, `allow_signup`, `initialized`) is the public
/// config consumed by the login/register/setup pages. The trailing `user` /
/// `telemetry` are the *session* slice: present for a signed-in caller, `null`
/// for an anonymous one. Folding them in here lets the layout boot the whole
/// app from a single request instead of fanning out to `/profile` and
/// `/client-config` as well.
#[derive(Debug, Serialize)]
pub struct AuthConfig {
    pub oauth_enabled: bool,
    pub oauth_provider_name: String,
    pub password_login_enabled: bool,
    pub allow_signup: bool,
    /// Whether the instance admin has been provisioned. When `false` the
    /// SPA routes the operator to `/setup`; when `true`, `/setup` bounces to login.
    pub initialized: bool,
    /// Current user when the request carries a valid session; `null` otherwise.
    pub user: Option<ProfileView>,
    /// Frontend telemetry (Sentry DSN). Only populated for an authenticated
    /// session, so the DSN never appears on an anonymous response.
    pub telemetry: Option<ClientConfigView>,
}

// ---------------------------------------------------------------------------
// /auth/config
// ---------------------------------------------------------------------------

/// `GET /auth/config` — bootstrap config for the SPA (public + session slice).
///
/// `password_login_enabled = !oauth_enabled`: when SSO is on the password form
/// is hidden (the built-in admin still reaches it via a direct link).
///
/// Optionally authenticated ([`OptionalAuthUser`]): an anonymous caller still
/// gets the public config (with `user`/`telemetry` `null`), while a signed-in
/// caller additionally receives their profile and telemetry config.
pub async fn auth_config(
    State(state): State<AppState>,
    OptionalAuthUser(current_user): OptionalAuthUser,
) -> Json<AuthConfig> {
    let oauth_enabled = state.oidc.is_some();
    let oauth_provider_name = state
        .oidc
        .as_ref()
        .map(|p| p.provider_name().to_string())
        .unwrap_or_default();
    // `allow_signup` is the persisted, UI-toggleable value (falls back to config).
    let allow_signup = state
        .settings
        .get()
        .await
        .map(|s| s.allow_signup)
        .unwrap_or(state.config.allow_signup);

    // First-run detection: the instance is "initialized" once an admin
    // exists. On a DB error default to `true` so we never expose `/setup` (and a
    // fresh admin creation) when the real state is unknown — `/auth/setup` itself
    // re-checks before creating, so this is purely a UI-routing hint.
    let initialized = state
        .users
        .count_admins()
        .await
        .map(|count| count > 0)
        .unwrap_or(true);

    // Telemetry is gated on the session so the DSN stays off anonymous responses.
    let telemetry = current_user
        .as_ref()
        .map(|_| ClientConfigView::from_state(&state));
    let user = current_user.map(ProfileView::from);

    Json(AuthConfig {
        oauth_enabled,
        oauth_provider_name,
        password_login_enabled: !oauth_enabled,
        allow_signup,
        initialized,
        user,
        telemetry,
    })
}

// ---------------------------------------------------------------------------
// /auth/oidc/login
// ---------------------------------------------------------------------------

/// `GET /auth/oidc/login` — start the SSO flow.
///
/// Returns `404` when SSO is disabled. Otherwise builds the authorize URL, sets
/// the signed transient `soika_oidc_state` cookie, and `302`-redirects to the
/// provider. A validated `?next=/path` is carried through the state cookie so
/// the callback can return the user where they were headed.
pub async fn oidc_login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let Some(provider) = state.oidc.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match build_login_redirect(&state, provider.as_ref(), query.next.as_deref()) {
        Ok(response) => response,
        // Pre-exchange failure (e.g. cookie signing); no cookie to clear yet.
        Err(err) => redirect_to_login(&err, None),
    }
}

/// Build the authorize-URL redirect with the signed state cookie attached.
fn build_login_redirect(
    state: &AppState,
    provider: &dyn super::oidc::OidcProvider,
    next: Option<&str>,
) -> Result<Response, Error> {
    let next = validated_next(next);

    let request = provider.authorize_url();
    let now = state.clock.now().timestamp();
    let signed = encode_state_cookie(
        &state.config.secret_key,
        request.csrf_state.secret(),
        request.nonce.secret(),
        request.pkce_verifier.secret(),
        next,
        now,
    )?;

    let secure = cookie_secure(&state.config);
    let cookie = build_state_cookie(&signed, secure);

    let mut headers = HeaderMap::new();
    push_cookie(&mut headers, &cookie)?;
    set_location(&mut headers, request.url.as_str())?;
    Ok((StatusCode::FOUND, headers).into_response())
}

// ---------------------------------------------------------------------------
// /auth/oidc/callback
// ---------------------------------------------------------------------------

/// `GET /auth/oidc/callback?code&state` — finish the SSO flow.
///
/// Validates the CSRF `state` against the signed cookie, exchanges the code,
/// requires `email_verified == true` + a non-empty allow-listed email, then
/// finds-or-creates the user, starts a session, clears the state cookie, and
/// redirects to `next`/`/`. Any flow error redirects to `/login?error=...`.
pub async fn oidc_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(provider) = state.oidc.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match run_callback(&state, provider.as_ref(), &headers, query).await {
        Ok(response) => response,
        // Flow failures are browser navigations: surface them on the login page
        // and always clear the transient state cookie.
        Err(err) => {
            let clearing = build_clearing_state_cookie(cookie_secure(&state.config));
            redirect_to_login(&err, Some(clearing))
        }
    }
}

/// The fallible body of [`oidc_callback`], factored out so the handler can map
/// any error to a single `/login?error=...` redirect.
async fn run_callback(
    state: &AppState,
    provider: &dyn super::oidc::OidcProvider,
    headers: &HeaderMap,
    query: CallbackQuery,
) -> Result<Response, Error> {
    let code = query
        .code
        .filter(|c| !c.is_empty())
        .ok_or_else(|| Error::Auth("missing authorization code".into()))?;
    let state_param = query
        .state
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Auth("missing state parameter".into()))?;

    let signed = read_cookie(headers, STATE_COOKIE)
        .ok_or_else(|| Error::Auth("missing OIDC state cookie".into()))?;
    let now = state.clock.now().timestamp();
    let payload = decode_state_cookie(&state.config.secret_key, &signed, now)?;

    // CSRF: the cookie-bound state must match the query state.
    if payload.state != state_param {
        return Err(Error::Auth("OIDC state mismatch".into()));
    }

    let claims = provider
        .exchange_code(
            code,
            PkceCodeVerifier::new(payload.pkce_verifier),
            Nonce::new(payload.nonce),
        )
        .await?;

    if !claims.email_verified {
        return Err(Error::Forbidden(
            "email is not verified by the provider".into(),
        ));
    }
    if claims.email.is_empty() {
        return Err(Error::Auth("provider returned an empty email".into()));
    }
    if !email_domain_allowed(&claims.email, provider.allowed_email_domains()) {
        return Err(Error::Forbidden("email domain is not allowed".into()));
    }

    // Find-or-create: an existing local account with the same email can also sign
    // in via SSO — except the built-in admin, which keeps password
    // login. Refusing the admin here closes an account-takeover
    // vector where the public IdP could assume the highest-privilege account by
    // email match.
    let user = match state.users.find_by_email(&claims.email).await? {
        Some(existing) if existing.is_admin => {
            return Err(Error::Forbidden(
                "admin account uses password login, not SSO".into(),
            ));
        }
        Some(existing) => existing,
        None => {
            state
                .users
                .create(NewUser {
                    email: claims.email,
                    display_name: claims.display_name,
                    password_hash: String::new(),
                    is_admin: false,
                    auth_provider: AuthProvider::Oidc,
                })
                .await?
        }
    };

    let (_, session_cookie) =
        start_session(&*state.sessions, &*state.clock, &state.config, user.id).await?;

    let secure = cookie_secure(&state.config);
    let target = payload.next.as_deref().unwrap_or("/");

    let mut headers_out = HeaderMap::new();
    push_cookie(&mut headers_out, &session_cookie)?;
    // Drop the transient state cookie now that the flow is complete.
    push_cookie(&mut headers_out, &build_clearing_state_cookie(secure))?;
    set_location(&mut headers_out, target)?;

    Ok((StatusCode::FOUND, headers_out).into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate a `?next` target against open-redirect: keep it only when it is a
/// site-relative path (starts with `/` but not `//`). Backslashes are
/// rejected outright — browsers normalize `\` to `/`, so `/\host` would slip
/// through as a protocol-relative URL.
fn validated_next(next: Option<&str>) -> Option<String> {
    let next = next?;
    if next.starts_with('/') && !next.starts_with("//") && !next.contains('\\') {
        return Some(next.to_string());
    }
    None
}

/// Read a single cookie value by name from the request `Cookie` header.
fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
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

fn push_cookie(headers: &mut HeaderMap, cookie: &str) -> Result<(), Error> {
    let value = HeaderValue::from_str(cookie)
        .map_err(|e| Error::internal(format!("invalid Set-Cookie value: {e}")))?;
    headers.append(SET_COOKIE, value);
    Ok(())
}

fn set_location(headers: &mut HeaderMap, location: &str) -> Result<(), Error> {
    let value = HeaderValue::from_str(location)
        .map_err(|e| Error::internal(format!("invalid Location value: {e}")))?;
    headers.insert(LOCATION, value);
    Ok(())
}

// ---------------------------------------------------------------------------
// Error → redirect mapping
// ---------------------------------------------------------------------------

/// Build the `/login?error=...` redirect, optionally clearing the state cookie.
fn redirect_to_login(err: &Error, clear_cookie: Option<String>) -> Response {
    let mut headers = HeaderMap::new();
    let location = format!("/login?error={}", url_encode(error_code(err)));
    // These header values are built from a fixed slug, so construction won't fail.
    if let Ok(value) = HeaderValue::from_str(&location) {
        headers.insert(LOCATION, value);
    }
    if let Some(cookie) = clear_cookie
        && let Ok(value) = HeaderValue::from_str(&cookie)
    {
        headers.append(SET_COOKIE, value);
    }
    (StatusCode::FOUND, headers).into_response()
}

/// Map an error to a short, stable, non-sensitive slug for the `?error=` param.
fn error_code(err: &Error) -> &'static str {
    match err {
        Error::Forbidden(_) => "forbidden",
        Error::Auth(_) => "auth_failed",
        Error::Validation(_) => "invalid_request",
        _ => "sso_failed",
    }
}

/// Minimal percent-encoding for the small set of slugs we emit (defensive: they
/// are already URL-safe, so this is effectively an identity for our inputs).
fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_next_accepts_site_relative_path() {
        assert_eq!(
            validated_next(Some("/dashboard")),
            Some("/dashboard".into())
        );
    }

    #[test]
    fn validated_next_rejects_protocol_relative_and_absolute() {
        assert_eq!(validated_next(Some("//evil.com")), None);
        assert_eq!(validated_next(Some("https://evil.com")), None);
        assert_eq!(validated_next(Some("evil")), None);
        assert_eq!(validated_next(None), None);
    }

    #[test]
    fn validated_next_rejects_backslash_path_confusion() {
        // Browsers normalize `\` to `/`, so `/\host` becomes protocol-relative.
        assert_eq!(validated_next(Some("/\\evil.com")), None);
        assert_eq!(validated_next(Some("\\\\evil.com")), None);
        assert_eq!(validated_next(Some("/path\\with\\backslash")), None);
    }

    #[test]
    fn error_code_is_stable_per_variant() {
        assert_eq!(error_code(&Error::Forbidden("x".into())), "forbidden");
        assert_eq!(error_code(&Error::Auth("x".into())), "auth_failed");
        assert_eq!(error_code(&Error::validation("x")), "invalid_request");
        assert_eq!(error_code(&Error::internal("x")), "sso_failed");
    }
}
