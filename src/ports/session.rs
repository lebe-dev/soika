//! Session repository port — server-side sessions.

use crate::domain::{Id, Session, Timestamp};
use crate::error::Result;
use async_trait::async_trait;

/// CRUD for opaque server-side sessions.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Persist a new session for `user_id`, expiring at `expires_at`.
    async fn create(&self, id: String, user_id: Id, expires_at: Timestamp) -> Result<Session>;
    /// Look up a session by its opaque id; expired sessions should not be returned.
    async fn find(&self, id: &str) -> Result<Option<Session>>;
    /// Delete a single session (logout).
    async fn delete(&self, id: &str) -> Result<()>;
    /// Delete all sessions for a user (e.g. on password change).
    async fn delete_for_user(&self, user_id: Id) -> Result<()>;
    /// Purge expired sessions (housekeeping).
    async fn delete_expired(&self, now: Timestamp) -> Result<u64>;
}
