//! Auth & invite HTTP handlers.
//!
//! These are the route bodies referenced by `crate::router`:
//!   * `POST /auth/login`     — authenticate, start a session, set the cookie.
//!   * `POST /auth/logout`    — destroy the session, clear the cookie.
//!   * `POST /auth/register`  — self-registration (only when `allow_signup`).
//!   * `GET  /invite/{token}` — inspect a pending invite.
//!   * `POST /invite/{token}` — accept an invite (existing user joins; new email
//!     registers then joins).
//!
//! Errors render as `{ "error": "..." }` JSON with the mapped HTTP status.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::domain::{Id, Invite, Role, User};
use crate::error::Error;
use crate::state::AppState;

use super::extractor::CurrentUser;
use super::invite_token::check_invite_usable;
use super::lockout::{LockoutDecision, client_ip, lockout_key};
use super::password::{hash_password, verify_password};
use super::session::{end_session, set_cookie_header, start_session};

// ---------------------------------------------------------------------------
// Error rendering
// ---------------------------------------------------------------------------

/// JSON error wrapper local to the auth module, mirroring `api::ApiError`'s
/// status mapping so auth responses are consistent with the rest of the API.
pub struct AuthError(pub Error);

impl From<Error> for AuthError {
    fn from(err: Error) -> Self {
        AuthError(err)
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = status_for(&self.0);
        (
            status,
            Json(ErrorBody {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}

fn status_for(err: &Error) -> StatusCode {
    match err {
        Error::NotFound(_) => StatusCode::NOT_FOUND,
        Error::Validation(_) => StatusCode::BAD_REQUEST,
        Error::Auth(_) => StatusCode::UNAUTHORIZED,
        Error::Forbidden(_) => StatusCode::FORBIDDEN,
        Error::Conflict(_) => StatusCode::CONFLICT,
        Error::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

/// `429 Too Many Requests` with a `Retry-After` hint, returned when the
/// brute-force guard has locked the (IP, email) key. Built directly rather than
/// via [`AuthError`] so the `Retry-After` header is included.
fn locked_response(retry_after_secs: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", retry_after_secs.to_string())],
        Json(ErrorBody {
            error: "too many failed attempts; try again later".to_string(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Client-safe view of a user (no password hash).
#[derive(Debug, Serialize)]
pub struct UserView {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    pub notifications_enabled: bool,
    /// Account origin: `"local"` or `"oidc"` (lets the UI hide "change password").
    pub auth_provider: crate::domain::AuthProvider,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        UserView {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            is_admin: u.is_admin,
            notifications_enabled: u.notifications_enabled,
            auth_provider: u.auth_provider,
        }
    }
}

/// `POST /auth/login` body.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// `POST /auth/register` body.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

/// `POST /auth/setup` body — first-run provisioning of the built-in admin.
#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub org_name: String,
}

/// `GET /invite/{token}` response — what the accept page needs to render.
#[derive(Debug, Serialize)]
pub struct InviteView {
    pub token: String,
    pub project_id: Id,
    pub role: Role,
    pub email: Option<String>,
    /// True when no account exists for the invite's target email, so the UI
    /// should prompt for registration rather than just "join".
    pub requires_registration: bool,
}

/// `POST /invite/{token}` body. For an existing/logged-in user the credential
/// fields may be omitted; a new email must supply password + display name.
#[derive(Debug, Default, Deserialize)]
pub struct AcceptInviteRequest {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn normalize_email(email: &str) -> Result<String, Error> {
    let e = email.trim().to_ascii_lowercase();
    if e.is_empty() || !e.contains('@') {
        return Err(Error::validation("a valid email is required"));
    }
    Ok(e)
}

fn validate_password(password: &str) -> Result<(), Error> {
    if password.len() < 8 {
        return Err(Error::validation("password must be at least 8 characters"));
    }
    Ok(())
}

fn validate_display_name(name: &str) -> Result<String, Error> {
    let n = name.trim();
    if n.is_empty() {
        return Err(Error::validation("display name must not be empty"));
    }
    Ok(n.to_string())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /auth/login` — verify credentials, create a session, set the cookie.
///
/// Brute-force protected: failures are counted per (client IP, email) and the
/// key is temporarily locked once they cross the configured threshold (see
/// [`super::lockout`]). The lock is checked *before* the argon2 verify so a
/// locked key can't be used to burn CPU.
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect: Option<ConnectInfo<SocketAddr>>,
    Json(body): Json<LoginRequest>,
) -> Result<Response, AuthError> {
    let email = normalize_email(&body.email)?;

    let ip = client_ip(&headers, connect.map(|c| c.0));
    let key = lockout_key(&ip, &email);
    if let LockoutDecision::Locked { retry_after_secs } = state.login_guard.check(&key) {
        return Ok(locked_response(retry_after_secs));
    }

    // Coexistence with SSO: when OAuth is enabled, password login is
    // reserved for the built-in admin (`is_admin`). Everyone else — including any
    // OIDC-provisioned account whose `password_hash` is the empty sentinel — must
    // use SSO. We still spend the same hashing effort on the rejected path so the
    // anti-enumeration timing profile is unchanged.
    let oauth_enabled = state.oidc.is_some();

    // Constant-ish work whether or not the user exists, to avoid user enumeration.
    let user = state.users.find_by_email(&email).await?;
    let ok = match &user {
        Some(u) => {
            let verified = verify_password(&body.password, &u.password_hash)?;
            // Under OAuth, non-admin accounts cannot log in with a password even
            // if their hash matched; reject after spending the verify effort.
            verified && (!oauth_enabled || u.is_admin)
        }
        None => {
            // Spend roughly the same effort on a dummy verify to reduce timing signal.
            let _ = verify_password(&body.password, dummy_hash());
            false
        }
    };

    if !ok {
        // Distinguish the "password login disabled" case for a known non-admin
        // account so the UI can point users at SSO, while keeping unknown emails
        // and bad passwords on the generic anti-enumeration path. This branch is
        // deterministic (independent of the password) so it does not count toward
        // the brute-force budget.
        if oauth_enabled
            && let Some(u) = &user
            && !u.is_admin
        {
            return Err(AuthError(Error::Forbidden(
                "password login is disabled; use SSO".into(),
            )));
        }
        // A genuine credential failure (bad password or unknown email): count it.
        state.login_guard.record_failure(&key);
        return Err(AuthError(Error::Auth("invalid email or password".into())));
    }
    let user = user.expect("ok implies a user was found");

    // Successful login clears any accrued failures / lockout for this key.
    state.login_guard.record_success(&key);

    let (_, cookie) =
        start_session(&*state.sessions, &*state.clock, &state.config, user.id).await?;

    let mut out = HeaderMap::new();
    set_cookie_header(&mut out, &cookie)?;
    Ok((out, Json(UserView::from(user))).into_response())
}

/// `POST /auth/setup` — first-run provisioning of the built-in admin.
///
/// Replaces the old `ADMIN_EMAIL`/`ADMIN_PASSWORD` env bootstrap: the instance
/// admin now lives in the database and is created here on first run. Allowed
/// only while the service is uninitialized (no admin exists); once an admin is
/// present this returns `409 Conflict` so the route can never be used to mint a
/// second privileged account. On success it creates the admin, persists the
/// organization name, starts a session and returns the admin — logging the
/// operator straight in.
pub async fn setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> Result<Response, AuthError> {
    // Gate on current state: refuse once any admin exists. This is the same
    // check the SPA uses via `/auth/config`, re-enforced server-side so the
    // endpoint is safe even if called directly.
    if state.users.count_admins().await? > 0 {
        return Err(AuthError(Error::Conflict(
            "the service is already initialized".into(),
        )));
    }

    let email = normalize_email(&body.email)?;
    validate_password(&body.password)?;
    let display_name = validate_display_name(&body.display_name)?;
    let org_name = body.org_name.trim();
    if org_name.is_empty() {
        return Err(AuthError(Error::validation(
            "organization name must not be empty",
        )));
    }

    let password_hash = hash_password(&body.password)?;
    let new = crate::ports::NewUser {
        email,
        display_name,
        password_hash,
        is_admin: true,
        auth_provider: crate::domain::AuthProvider::Local,
    };
    // A UNIQUE(email) conflict here means a concurrent setup won the race; map it
    // to the same "already initialized" outcome rather than a raw error.
    let user = match state.users.create(new).await {
        Ok(user) => user,
        Err(Error::Conflict(_)) => {
            return Err(AuthError(Error::Conflict(
                "the service is already initialized".into(),
            )));
        }
        Err(e) => return Err(AuthError(e)),
    };

    state.settings.set_org_name(org_name.to_string()).await?;

    let (_, cookie) =
        start_session(&*state.sessions, &*state.clock, &state.config, user.id).await?;

    let mut headers = HeaderMap::new();
    set_cookie_header(&mut headers, &cookie)?;
    Ok((StatusCode::CREATED, headers, Json(UserView::from(user))).into_response())
}

/// `POST /auth/logout` — destroy the current session and clear the cookie.
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AuthError> {
    let clearing = end_session(&*state.sessions, &state.config, &headers).await?;
    let mut out = HeaderMap::new();
    set_cookie_header(&mut out, &clearing)?;
    Ok((out, StatusCode::NO_CONTENT).into_response())
}

/// `POST /auth/register` — self-registration, only when `allow_signup` is on.
///
/// `allow_signup` is read from the persisted service settings (UI-toggleable,
/// ), falling back to the config default. Invite-based registration is a
/// separate path (`accept_invite`) and is NOT gated by this flag.
pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<Response, AuthError> {
    // Coexistence with SSO: when OAuth is enabled, self-registration
    // is disabled regardless of `allow_signup` — new accounts are provisioned via
    // SSO sign-in. Admin is bootstrapped at startup, not via this endpoint.
    if state.oidc.is_some() {
        return Err(AuthError(Error::Forbidden(
            "self-registration is disabled; use SSO".into(),
        )));
    }

    let allow_signup = state.settings.get().await?.allow_signup;
    if !allow_signup {
        return Err(AuthError(Error::Forbidden(
            "public sign-up is disabled".into(),
        )));
    }

    let user = create_user(&state, &body.email, &body.password, &body.display_name).await?;
    let (_, cookie) =
        start_session(&*state.sessions, &*state.clock, &state.config, user.id).await?;

    let mut headers = HeaderMap::new();
    set_cookie_header(&mut headers, &cookie)?;
    Ok((StatusCode::CREATED, headers, Json(UserView::from(user))).into_response())
}

/// `GET /invite/{token}` — inspect a pending invite so the UI can render the
/// accept flow. Returns `401` for expired/accepted/unknown tokens.
pub async fn get_invite(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<InviteView>, AuthError> {
    let invite = load_usable_invite(&state, &token).await?;

    let requires_registration = match &invite.email {
        Some(email) => state.users.find_by_email(email).await?.is_none(),
        None => false,
    };

    Ok(Json(InviteView {
        token: invite.token,
        project_id: invite.project_id,
        role: invite.role,
        email: invite.email,
        requires_registration,
    }))
}

/// `POST /invite/{token}` — accept an invite.
///
/// Three cases:
///   1. A logged-in user → joins the project with the invite's role.
///   2. An existing account (by email + password) → authenticates, then joins.
///   3. A new email → registers (independent of `allow_signup`), then joins.
///
/// The anonymous branch verifies a password (case 2), so it is brute-force
/// protected the same way as `login`. A logged-in acceptance (case 1) verifies
/// no credentials and is not gated.
pub async fn accept_invite(
    State(state): State<AppState>,
    current: Option<CurrentUser>,
    Path(token): Path<String>,
    headers: HeaderMap,
    connect: Option<ConnectInfo<SocketAddr>>,
    body: Option<Json<AcceptInviteRequest>>,
) -> Result<Response, AuthError> {
    let invite = load_usable_invite(&state, &token).await?;
    let req = body.map(|Json(b)| b).unwrap_or_default();

    // Resolve which user is accepting, plus whether we minted a new session.
    let (user, set_cookie): (User, Option<String>) = if let Some(CurrentUser(u)) = current {
        (u, None)
    } else {
        // Key the guard on the candidate email (request, else invite target) so
        // a password-guessing loop against an existing account is throttled.
        let candidate_email = req
            .email
            .as_deref()
            .or(invite.email.as_deref())
            .unwrap_or("unknown")
            .trim()
            .to_ascii_lowercase();
        let ip = client_ip(&headers, connect.map(|c| c.0));
        let key = lockout_key(&ip, &candidate_email);
        if let LockoutDecision::Locked { retry_after_secs } = state.login_guard.check(&key) {
            return Ok(locked_response(retry_after_secs));
        }

        match resolve_or_register_invitee(&state, &invite, &req).await {
            Ok((u, cookie)) => {
                state.login_guard.record_success(&key);
                (u, Some(cookie))
            }
            // Only a credential failure feeds the brute-force budget; validation
            // / forbidden errors are deterministic and pass straight through.
            Err(e @ Error::Auth(_)) => {
                state.login_guard.record_failure(&key);
                return Err(AuthError(e));
            }
            Err(e) => return Err(AuthError(e)),
        }
    };

    // Join the project with the invited role (idempotent upsert).
    state
        .memberships
        .upsert(invite.project_id, user.id, invite.role)
        .await?;
    state
        .invites
        .mark_accepted(&invite.token, state.clock.now())
        .await?;

    let view = Json(UserView::from(user));
    match set_cookie {
        Some(cookie) => {
            let mut headers = HeaderMap::new();
            set_cookie_header(&mut headers, &cookie)?;
            Ok((headers, view).into_response())
        }
        None => Ok(view.into_response()),
    }
}

// ---------------------------------------------------------------------------
// Shared logic
// ---------------------------------------------------------------------------

/// Create a user with validated inputs and a hashed password. Maps a duplicate
/// email to `Error::Conflict`.
async fn create_user(
    state: &AppState,
    email: &str,
    password: &str,
    display_name: &str,
) -> Result<User, Error> {
    let email = normalize_email(email)?;
    validate_password(password)?;
    let display_name = validate_display_name(display_name)?;

    if state.users.find_by_email(&email).await?.is_some() {
        return Err(Error::Conflict(
            "an account with this email already exists".into(),
        ));
    }

    let password_hash = hash_password(password)?;
    let new = crate::ports::NewUser {
        email,
        display_name,
        password_hash,
        is_admin: false,
        auth_provider: crate::domain::AuthProvider::Local,
    };
    state.users.create(new).await
}

/// Load an invite by token and ensure it is still usable (unexpired, unaccepted).
async fn load_usable_invite(state: &AppState, token: &str) -> Result<Invite, Error> {
    let invite = state
        .invites
        .find_by_token(token)
        .await?
        .ok_or_else(|| Error::Auth("unknown invite".into()))?;
    check_invite_usable(&invite, state.clock.now())?;
    Ok(invite)
}

/// For an anonymous invite acceptance, either authenticate an existing account
/// or register a new one, returning the user plus the session cookie to set.
async fn resolve_or_register_invitee(
    state: &AppState,
    invite: &Invite,
    req: &AcceptInviteRequest,
) -> Result<(User, String), Error> {
    let email = req
        .email
        .as_deref()
        .or(invite.email.as_deref())
        .ok_or_else(|| Error::validation("email is required to accept this invite"))?;
    let email = normalize_email(email)?;
    let password = req
        .password
        .as_deref()
        .ok_or_else(|| Error::validation("password is required to accept this invite"))?;

    let user = match state.users.find_by_email(&email).await? {
        Some(existing) => {
            // Existing account → must authenticate before joining.
            if !verify_password(password, &existing.password_hash)? {
                return Err(Error::Auth("invalid email or password".into()));
            }
            existing
        }
        None => {
            // Coexistence with SSO: under OAuth we do not mint a
            // password-backed account from an invite. Behaviour: the invitee
            // must first sign in via SSO (which find-or-creates their account),
            // then accept the invite while logged in. We surface this as a
            // Forbidden so the UI can route them to SSO. Chosen for simplicity —
            // no OIDC-from-invite onboarding in the first pass.
            if state.oidc.is_some() {
                return Err(Error::Forbidden(
                    "sign in with SSO first, then accept this invite".into(),
                ));
            }
            // New email → register (invite authorizes regardless of allow_signup).
            let display_name = req.display_name.as_deref().unwrap_or(&email);
            create_user(state, &email, password, display_name).await?
        }
    };

    let (_, cookie) =
        start_session(&*state.sessions, &*state.clock, &state.config, user.id).await?;
    Ok((user, cookie))
}

/// A valid argon2 PHC hash of a fixed string, computed once and reused as a
/// timing-equalizer for logins against non-existent accounts. Verifying a real
/// hash here keeps the work comparable to the happy path, mitigating user
/// enumeration via response timing. Verification always fails for real inputs.
fn dummy_hash() -> &'static str {
    use std::sync::OnceLock;
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY
        .get_or_init(|| {
            hash_password("timing-equalizer-not-a-real-password")
                .expect("hashing a fixed non-empty string cannot fail")
        })
        .as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_email_lowercases_and_trims() {
        assert_eq!(normalize_email("  Foo@Bar.COM ").unwrap(), "foo@bar.com");
        assert!(normalize_email("not-an-email").is_err());
        assert!(normalize_email("   ").is_err());
    }

    #[test]
    fn password_minimum_length_enforced() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("longenough").is_ok());
    }

    #[test]
    fn display_name_must_not_be_blank() {
        assert_eq!(validate_display_name("  Jane ").unwrap(), "Jane");
        assert!(validate_display_name("   ").is_err());
    }

    #[test]
    fn error_status_mapping_is_consistent() {
        assert_eq!(
            status_for(&Error::Auth("x".into())),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status_for(&Error::Forbidden("x".into())),
            StatusCode::FORBIDDEN
        );
        assert_eq!(status_for(&Error::validation("x")), StatusCode::BAD_REQUEST);
        assert_eq!(
            status_for(&Error::Conflict("x".into())),
            StatusCode::CONFLICT
        );
        assert_eq!(status_for(&Error::not_found("x")), StatusCode::NOT_FOUND);
    }
}
