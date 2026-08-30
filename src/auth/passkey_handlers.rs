//! Passkey HTTP handlers: the unauthenticated `/auth/passkey/*` sign-in
//! ceremony and the session-authenticated `/api/passkeys*` management routes.
//!
//! Sign-in is **usernameless**: the browser discovers which credential (and
//! therefore which account) to use, so no email is submitted and no account
//! enumeration is possible. The WebAuthn user handle is the `users.id` UUID, so
//! an assertion resolves straight to an account.
//!
//! Both routes are `404` while `PASSKEY_ENABLED` is off — the feature is either
//! configured or absent, and a disabled instance should not advertise it.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use webauthn_rs::Webauthn;
use webauthn_rs::prelude::{
    CreationChallengeResponse, DiscoverableAuthentication, DiscoverableKey,
    Passkey as WebauthnPasskey, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, RequestChallengeResponse,
};

use crate::api::teams::{ApiError, AuthUser};
use crate::config::PasskeyConfig;
use crate::domain::{Id, Passkey, Timestamp, normalize_passkey_name};
use crate::error::{Error, Result};
use crate::state::AppState;

use super::extractor::PeerAddr;
use super::handlers::{AuthError, UserView};
use super::lockout::{LockoutDecision, client_ip, lockout_key};
use super::passkey::{
    PASSKEY_AUTHENTICATION_COOKIE, PASSKEY_REGISTRATION_COOKIE, build_challenge_cookie,
    build_clearing_challenge_cookie, credential_id_b64, decode_challenge, decode_credential_id,
    encode_challenge, encode_credential_id,
};
use super::session::{cookie_secure, start_session};

/// Label used when a client registers a credential without naming it.
const DEFAULT_PASSKEY_NAME: &str = "Passkey";

/// Lockout bucket for passkey sign-in. The ceremony carries no email, so the
/// brute-force guard is keyed per client IP with this constant in place of one.
const PASSKEY_LOCKOUT_SUBJECT: &str = "passkey";

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// Client-safe view of a registered credential (never exposes the public key).
#[derive(Debug, Serialize)]
pub struct PasskeyView {
    pub id: Id,
    pub name: String,
    pub created_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
}

impl From<Passkey> for PasskeyView {
    fn from(key: Passkey) -> Self {
        PasskeyView {
            id: key.id,
            name: key.name,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
        }
    }
}

/// `POST /api/passkeys/register` body: the browser's attestation response plus
/// the label the user gave the credential.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub credential: RegisterPublicKeyCredential,
}

/// `PATCH /api/passkeys/{id}` body.
#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub name: String,
}

// ---------------------------------------------------------------------------
// Sign-in ceremony (unauthenticated)
// ---------------------------------------------------------------------------

/// `POST /auth/passkey/login/options` — start a usernameless assertion.
///
/// Returns the WebAuthn request options and stores the ceremony state in a
/// signed, short-lived cookie. No account is identified at this point, so the
/// response is identical whether or not any passkey exists.
pub async fn login_options(
    State(state): State<AppState>,
    headers: HeaderMap,
    PeerAddr(peer): PeerAddr,
) -> std::result::Result<Response, AuthError> {
    let (webauthn, config) = require_passkeys(&state)?;

    // The options endpoint is cheap but not free; honour the same lockout the
    // verification step feeds so a blocked client cannot keep minting challenges.
    let ip = client_ip(&headers, peer);
    let key = lockout_key(&ip, PASSKEY_LOCKOUT_SUBJECT);
    if let LockoutDecision::Locked { retry_after_secs } = state.login_guard.check(&key) {
        tracing::warn!(client_ip = %ip, retry_after_secs, "passkey login blocked: IP temporarily locked");
        return Err(AuthError(Error::RateLimited));
    }

    let (challenge, ceremony) = webauthn
        .start_discoverable_authentication()
        .map_err(webauthn_start_error)?;

    let cookie = challenge_cookie(
        &state,
        config,
        PASSKEY_AUTHENTICATION_COOKIE,
        &ceremony,
        None,
    )?;

    Ok(with_cookie(Json(strip_mediation(challenge)), &cookie)?)
}

/// `POST /auth/passkey/login` — finish the assertion and start a session.
pub async fn login_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    PeerAddr(peer): PeerAddr,
    Json(credential): Json<PublicKeyCredential>,
) -> std::result::Result<Response, AuthError> {
    let (webauthn, _) = require_passkeys(&state)?;
    let secure = cookie_secure(&state.config);

    let ip = client_ip(&headers, peer);
    let key = lockout_key(&ip, PASSKEY_LOCKOUT_SUBJECT);
    if let LockoutDecision::Locked { retry_after_secs } = state.login_guard.check(&key) {
        tracing::warn!(client_ip = %ip, retry_after_secs, "passkey login blocked: IP temporarily locked");
        return Err(AuthError(Error::RateLimited));
    }

    let signed = read_cookie(&headers, PASSKEY_AUTHENTICATION_COOKIE)
        .ok_or_else(|| Error::Auth("no passkey challenge in progress".into()))?;
    let payload = decode_challenge::<DiscoverableAuthentication>(
        &state.config.secret_key,
        &signed,
        now_unix(&state),
    )?;

    // The assertion names its own account via the user handle; everything below
    // is verified against the credentials that account actually owns.
    let (user_id, credential_id) = webauthn
        .identify_discoverable_authentication(&credential)
        .map_err(|_| Error::Auth("passkey is not recognised".into()))?;
    let credential_id = encode_credential_id(credential_id);

    let stored = state
        .passkeys
        .find_by_credential_id(&credential_id)
        .await?
        .filter(|key| key.user_id == user_id);
    let Some(stored) = stored else {
        state.login_guard.record_failure(&key);
        return Err(AuthError(Error::Auth("passkey is not recognised".into())));
    };

    let mut verified: WebauthnPasskey = parse_credential(&stored)?;
    let discoverable: Vec<DiscoverableKey> = vec![DiscoverableKey::from(&verified)];

    let result = match webauthn.finish_discoverable_authentication(
        &credential,
        payload.state,
        &discoverable,
    ) {
        Ok(result) => result,
        Err(err) => {
            // A failed assertion counts toward the brute-force budget, and the
            // spent challenge cookie is dropped so the ceremony must restart.
            state.login_guard.record_failure(&key);
            tracing::warn!(client_ip = %ip, error = %err, "passkey assertion failed");
            let response = with_cookie(
                AuthError(Error::Auth("passkey verification failed".into())),
                &build_clearing_challenge_cookie(PASSKEY_AUTHENTICATION_COOKIE, secure),
            )?;
            return Ok(response);
        }
    };

    let user = state
        .users
        .find_by_id(user_id)
        .await?
        .ok_or_else(|| Error::Auth("passkey is not recognised".into()))?;

    // A pending account holds no session until an instance admin approves it —
    // the same gate the SSO callback applies.
    if user.status.is_pending() {
        return Err(AuthError(Error::Forbidden(
            "account is awaiting approval".into(),
        )));
    }

    // Persist the post-assertion credential state (signature counter, backup
    // flags) so a cloned authenticator is detectable on the next sign-in.
    verified.update_credential(&result);
    let serialized = serde_json::to_string(&verified).map_err(Error::from)?;
    state
        .passkeys
        .touch(stored.id, &serialized, state.clock.now())
        .await?;

    state.login_guard.record_success(&key);

    let (_, session_cookie) =
        start_session(&*state.sessions, &*state.clock, &state.config, user.id).await?;

    tracing::info!(user_id = %user.id, client_ip = %ip, "passkey login succeeded");

    let mut out = HeaderMap::new();
    push_cookie(&mut out, &session_cookie)?;
    push_cookie(
        &mut out,
        &build_clearing_challenge_cookie(PASSKEY_AUTHENTICATION_COOKIE, secure),
    )?;
    Ok((out, Json(UserView::from(user))).into_response())
}

// ---------------------------------------------------------------------------
// Management (session-authenticated)
// ---------------------------------------------------------------------------

/// `GET /api/passkeys` — the caller's registered credentials, newest first.
pub async fn list(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> std::result::Result<Json<Vec<PasskeyView>>, ApiError> {
    require_passkeys(&state)?;
    let keys = state.passkeys.list_for_user(user.id).await?;
    Ok(Json(keys.into_iter().map(PasskeyView::from).collect()))
}

/// `POST /api/passkeys/options` — start registering a new credential.
///
/// Credentials the caller already registered are sent as `excludeCredentials`
/// so an authenticator refuses to enrol the same key twice, and the options are
/// upgraded to require a *discoverable* credential — the sign-in flow is
/// usernameless and only resident keys can serve it.
pub async fn register_options(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> std::result::Result<Response, ApiError> {
    let (webauthn, config) = require_passkeys(&state)?;

    let registered = state.passkeys.count_for_user(user.id).await?;
    if registered >= config.max_per_user {
        return Err(ApiError(Error::validation(format!(
            "at most {} passkeys can be registered per user",
            config.max_per_user
        ))));
    }

    // The exclusion list is built from the stored credential ids alone — no need
    // to deserialize each credential just to name it.
    let existing = state.passkeys.list_for_user(user.id).await?;
    let exclude = existing
        .iter()
        .map(|key| decode_credential_id(&key.credential_id))
        .collect::<Result<Vec<_>>>()?;

    let (challenge, ceremony) = webauthn
        .start_passkey_registration(user.id, &user.email, &user.display_name, Some(exclude))
        .map_err(webauthn_start_error)?;

    let cookie = challenge_cookie(
        &state,
        config,
        PASSKEY_REGISTRATION_COOKIE,
        &ceremony,
        Some(user.id),
    )?;

    Ok(with_cookie(Json(require_discoverable(challenge)), &cookie)?)
}

/// `POST /api/passkeys` — finish registration and store the credential.
pub async fn register_verify(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> std::result::Result<Response, ApiError> {
    let (webauthn, config) = require_passkeys(&state)?;
    let secure = cookie_secure(&state.config);

    let name = normalize_passkey_name(
        body.name.as_deref().unwrap_or_default(),
        DEFAULT_PASSKEY_NAME,
    )
    .map_err(Error::validation)?;

    let signed = read_cookie(&headers, PASSKEY_REGISTRATION_COOKIE)
        .ok_or_else(|| Error::Auth("no passkey registration in progress".into()))?;
    let payload = decode_challenge::<PasskeyRegistration>(
        &state.config.secret_key,
        &signed,
        now_unix(&state),
    )?;

    // The ceremony is bound to the account that started it: a state minted for
    // another session can never enrol a credential here.
    if payload.user_id != Some(user.id) {
        return Err(ApiError(Error::Auth(
            "passkey registration does not belong to this session".into(),
        )));
    }

    let credential = webauthn
        .finish_passkey_registration(&body.credential, &payload.state)
        .map_err(|err| {
            tracing::warn!(user_id = %user.id, error = %err, "passkey registration failed");
            Error::validation("passkey registration could not be verified")
        })?;

    // Re-check the quota: `options` and this call are separate requests.
    if state.passkeys.count_for_user(user.id).await? >= config.max_per_user {
        return Err(ApiError(Error::validation(format!(
            "at most {} passkeys can be registered per user",
            config.max_per_user
        ))));
    }

    let stored = state
        .passkeys
        .create(crate::ports::NewPasskey {
            user_id: user.id,
            credential_id: credential_id_b64(&credential),
            name,
            credential: serde_json::to_string(&credential).map_err(Error::from)?,
        })
        .await?;

    tracing::info!(user_id = %user.id, passkey_id = %stored.id, "passkey registered");

    Ok(with_cookie(
        Json(PasskeyView::from(stored)),
        &build_clearing_challenge_cookie(PASSKEY_REGISTRATION_COOKIE, secure),
    )?)
}

/// `PATCH /api/passkeys/{id}` — rename one of the caller's credentials.
pub async fn rename(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Id>,
    Json(body): Json<RenameRequest>,
) -> std::result::Result<Json<PasskeyView>, ApiError> {
    require_passkeys(&state)?;
    let name =
        normalize_passkey_name(&body.name, DEFAULT_PASSKEY_NAME).map_err(Error::validation)?;
    let renamed = state.passkeys.rename(user.id, id, &name).await?;
    Ok(Json(PasskeyView::from(renamed)))
}

/// `DELETE /api/passkeys/{id}` — remove one of the caller's credentials.
pub async fn delete(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Id>,
) -> std::result::Result<StatusCode, ApiError> {
    require_passkeys(&state)?;
    state.passkeys.delete(user.id, id).await?;
    tracing::info!(user_id = %user.id, passkey_id = %id, "passkey removed");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The relying party and its config, or `NotFound` (→ `404`) when passkeys are
/// disabled on this instance.
fn require_passkeys(state: &AppState) -> Result<(&Webauthn, &PasskeyConfig)> {
    let webauthn = state
        .webauthn
        .as_deref()
        .ok_or_else(|| Error::not_found("passkeys are not enabled"))?;
    let config = state
        .config
        .passkey
        .as_ref()
        .ok_or_else(|| Error::not_found("passkeys are not enabled"))?;
    Ok((webauthn, config))
}

/// Sign a ceremony state and wrap it in its `Set-Cookie` value.
fn challenge_cookie<S: serde::Serialize>(
    state: &AppState,
    config: &PasskeyConfig,
    name: &str,
    ceremony: &S,
    user_id: Option<Id>,
) -> Result<String> {
    let ttl_secs = config.challenge_ttl.as_secs() as i64;
    let signed = encode_challenge(
        &state.config.secret_key,
        ceremony,
        user_id,
        now_unix(state),
        ttl_secs,
    )?;
    Ok(build_challenge_cookie(
        name,
        &signed,
        ttl_secs,
        cookie_secure(&state.config),
    ))
}

/// Force a *discoverable* credential: the library's passkey defaults leave the
/// resident-key requirement at "discouraged", which would produce a credential
/// the usernameless sign-in flow cannot find.
fn require_discoverable(mut challenge: CreationChallengeResponse) -> CreationChallengeResponse {
    use webauthn_rs_core::proto::ResidentKeyRequirement;

    if let Some(selection) = challenge.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(ResidentKeyRequirement::Required);
        selection.require_resident_key = true;
    }
    challenge
}

/// Drop the library's `mediation: conditional` hint: soika's sign-in is a
/// button, not an autofill affordance, so the browser must show its own picker.
fn strip_mediation(mut challenge: RequestChallengeResponse) -> RequestChallengeResponse {
    challenge.mediation = None;
    challenge
}

/// Deserialize a stored credential back into the library's type.
fn parse_credential(stored: &Passkey) -> Result<WebauthnPasskey> {
    serde_json::from_str(&stored.credential)
        .map_err(|e| Error::internal(format!("stored passkey is unreadable: {e}")))
}

fn webauthn_start_error(err: webauthn_rs::prelude::WebauthnError) -> Error {
    Error::internal(format!("could not start the passkey ceremony: {err}"))
}

fn now_unix(state: &AppState) -> i64 {
    state.clock.now().timestamp()
}

/// Attach a `Set-Cookie` header to an already-built response.
fn with_cookie(body: impl IntoResponse, cookie: &str) -> Result<Response> {
    let mut headers = HeaderMap::new();
    push_cookie(&mut headers, cookie)?;
    Ok((headers, body).into_response())
}

fn push_cookie(headers: &mut HeaderMap, cookie: &str) -> Result<()> {
    let value = axum::http::HeaderValue::from_str(cookie)
        .map_err(|e| Error::internal(format!("invalid Set-Cookie value: {e}")))?;
    headers.append(axum::http::header::SET_COOKIE, value);
    Ok(())
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
