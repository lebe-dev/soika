//! Authentication, sessions, roles, invites & admin bootstrap (MVP §9–§11).
//!
//! Architecture:
//!   * [`password`] — argon2id hashing/verification.
//!   * [`signing`] — HMAC-SHA256 signing for tamper-evident cookies/tokens.
//!   * [`session`] — server-side session lifecycle + the signed session cookie.
//!   * [`extractor`] — the [`CurrentUser`] axum extractor and per-project role
//!     gating ([`require_project_admin`], [`require_project_member`], …).
//!   * [`invite_token`] — invite token generation, links, and usability checks.
//!   * [`handlers`] — the `/auth/*` and `/invite/{token}` route bodies.
//!   * [`bootstrap`] — idempotent built-in admin provisioning at startup (§11).
//!
//! Sessions are server-side (no JWT): the cookie carries only an opaque, signed
//! session id; the authoritative record lives in the DB via `SessionRepository`.

pub mod bootstrap;
pub mod extractor;
pub mod handlers;
pub mod invite_token;
pub mod oidc;
pub mod oidc_handlers;
pub mod password;
pub mod session;
pub mod signing;

// --- Router-facing handler surface (referenced by `crate::router`) -----------
pub use handlers::{accept_invite, get_invite, login, logout, register};

// --- OIDC handler surface (referenced by `crate::router`, PLAN §6.6) ---------
pub use oidc_handlers::{AuthConfig, auth_config, oidc_callback, oidc_login};

// --- Password helpers (used by handlers, bootstrap, profile updates) ---------
pub use password::{hash_password, verify_password};

// --- Authorization surface (used by the internal API handlers) ---------------
pub use extractor::{
    AuthRejection, CurrentUser, authorize_project, require_instance_admin, require_project_admin,
    require_project_member, role_satisfies,
};

// --- Session surface (used by handlers; also by other agents if needed) ------
pub use session::{
    SESSION_COOKIE, SESSION_TTL_DAYS, build_clearing_cookie, build_session_cookie, cookie_secure,
    end_session, session_id_from_headers, set_cookie_header, start_session,
};

// --- OIDC surface (PLAN §6.1/§6.2; used by oidc handlers in Story 4) ---------
pub use oidc::{
    AuthorizeRequest, OidcClaims, OidcClient, OidcProvider, STATE_COOKIE, StatePayload,
    build_clearing_state_cookie, build_state_cookie, decode_state_cookie, email_domain_allowed,
    encode_state_cookie,
};

// --- Invite surface ----------------------------------------------------------
pub use invite_token::{INVITE_TTL_DAYS, check_invite_usable, generate_invite_token, invite_link};

// --- Bootstrap (called from `main.rs` on startup, §11) -----------------------
pub use bootstrap::{BootstrapOutcome, bootstrap_admin};
