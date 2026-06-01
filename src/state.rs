//! Shared application state (`AppState`) wired into the axum router.
//!
//! Holds an `Arc<dyn ...Repository>` for each port plus `Config`, the `Mailer`
//! and the `Clock`. Owned by foundation; feature agents read it from handlers
//! but should not change its shape.

use std::sync::Arc;

use crate::config::Config;
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
}
