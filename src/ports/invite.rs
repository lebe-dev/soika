//! Invite repository port.

use crate::domain::{Id, Invite, Role, Timestamp};
use crate::error::Result;
use async_trait::async_trait;

/// Data required to create an invite.
#[derive(Debug, Clone)]
pub struct NewInvite {
    pub token: String,
    pub project_id: Id,
    pub role: Role,
    pub email: Option<String>,
    pub created_by: Option<Id>,
    pub expires_at: Timestamp,
}

/// CRUD for project invites.
#[async_trait]
pub trait InviteRepository: Send + Sync {
    async fn create(&self, new: NewInvite) -> Result<Invite>;
    /// Look up an invite by its token (`/invite/{token}` flow).
    async fn find_by_token(&self, token: &str) -> Result<Option<Invite>>;
    /// Pending invites for a project.
    async fn list_for_project(&self, project_id: Id) -> Result<Vec<Invite>>;
    /// Mark an invite accepted at the given time.
    async fn mark_accepted(&self, token: &str, accepted_at: Timestamp) -> Result<()>;
    async fn delete(&self, token: &str) -> Result<()>;
}
