//! Shared application state (`AppState`) wired into the axum router.
//!
//! Holds an `Arc<dyn ...Repository>` for each port plus `Config`, the `Mailer`
//! and the `Clock`. Owned by foundation; feature agents read it from handlers
//! but should not change its shape.

use std::sync::Arc;

use crate::auth::{LoginGuard, OidcProvider};
use crate::config::Config;
use crate::ingest::RateLimiter;
use crate::ports::{
    Clock, EventRepository, InviteRepository, IssueRepository, Mailer, MembershipRepository,
    ProjectRepository, SessionRepository, SettingsRepository, TeamRepository, UserRepository,
};

/// Application state shared across all handlers (cheaply cloneable).
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub users: Arc<dyn UserRepository>,
    pub sessions: Arc<dyn SessionRepository>,
    pub teams: Arc<dyn TeamRepository>,
    pub memberships: Arc<dyn MembershipRepository>,
    pub projects: Arc<dyn ProjectRepository>,
    pub issues: Arc<dyn IssueRepository>,
    pub events: Arc<dyn EventRepository>,
    pub invites: Arc<dyn InviteRepository>,
    pub settings: Arc<dyn SettingsRepository>,
    pub mailer: Arc<dyn Mailer>,
    pub clock: Arc<dyn Clock>,
    /// Soft per-project ingestion rate limiter. Owned by the state so it
    /// is per-instance (and configurable in tests) rather than process-global.
    pub rate_limiter: Arc<RateLimiter>,
    /// Brute-force guard for password logins, keyed by (client IP, email).
    /// Per-instance like `rate_limiter`; configured from `Config::lockout`.
    pub login_guard: Arc<LoginGuard>,
    /// OIDC provider, present only when SSO is enabled. Built once at
    /// startup via discovery (fail-fast); `None` keeps password login unchanged.
    pub oidc: Option<Arc<dyn OidcProvider>>,
}

impl AppState {
    /// Attach an OIDC provider (built at startup via discovery),
    /// returning the updated state. `None` leaves SSO disabled.
    pub fn with_oidc(mut self, oidc: Option<Arc<dyn OidcProvider>>) -> Self {
        self.oidc = oidc;
        self
    }
}
