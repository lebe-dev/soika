//! Authentication, sessions, roles, invites & admin bootstrap.
//!
//! Architecture:
//!   * [`password`] — argon2id hashing/verification.
//!   * [`signing`] — HMAC-SHA256 signing for tamper-evident cookies/tokens.
//!   * [`session`] — server-side session lifecycle + the signed session cookie.
//!   * [`extractor`] — the [`CurrentUser`] axum extractor and the instance-role
//!     gates ([`require_manager`], [`require_owner`]). Per-project authorization
//!     lives in [`crate::api::projects::effective_role`] (team-aware), not here.
//!   * [`invite_token`] — invite token generation, links, and usability checks.
//!   * [`handlers`] — the `/auth/*` and `/invite/{token}` route bodies.
//!
//! Sessions are server-side (no JWT): the cookie carries only an opaque, signed
//! session id; the authoritative record lives in the DB via `SessionRepository`.

pub mod extractor;
pub mod github;
pub mod handlers;
pub mod invite_token;
pub mod lockout;
pub mod oidc;
pub mod oidc_handlers;
pub mod password;
pub mod session;
pub mod signing;

// --- Router-facing handler surface (referenced by `crate::router`) -----------
pub use handlers::{accept_invite, get_invite, login, logout, register, setup};

// --- OIDC handler surface (referenced by `crate::router`) ---------
pub use oidc_handlers::{AuthConfig, auth_config, oidc_callback, oidc_login};

// --- Password helpers (used by handlers, bootstrap, profile updates) ---------
pub use password::{hash_password, verify_password};

// --- Authentication surface (used by the internal API handlers) --------------
//
// Per-project authorization is NOT re-exported here: the single source of that
// rule is `crate::api::projects::effective_role` (team-aware). This module
// exposes only authentication (`CurrentUser`) and the instance-role gates.
pub use extractor::{AuthRejection, CurrentUser, require_manager, require_owner};

// --- Session surface (used by handlers; also by other agents if needed) ------
pub use session::{
    SESSION_COOKIE, SESSION_TTL_DAYS, build_clearing_cookie, build_session_cookie, cookie_secure,
    end_session, session_id_from_headers, set_cookie_header, start_session,
};

// --- OIDC surface (used by oidc handlers in) ---------
pub use github::GithubProvider;
pub use oidc::{
    AuthorizeRequest, OidcClaims, OidcClient, OidcProvider, STATE_COOKIE, StatePayload,
    build_clearing_state_cookie, build_state_cookie, decode_state_cookie, email_domain_allowed,
    encode_state_cookie,
};

// --- Invite surface ----------------------------------------------------------
pub use invite_token::{INVITE_TTL_DAYS, check_invite_usable, generate_invite_token, invite_link};

// --- Brute-force protection (used by handlers; wired into AppState) -----------
pub use lockout::{LockoutDecision, LoginGuard, client_ip, lockout_key};
