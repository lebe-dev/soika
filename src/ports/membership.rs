//! Membership repository port — User × Project × Role.

use crate::domain::{Id, Membership, Role, User};
use crate::error::Result;
use async_trait::async_trait;

/// Manage per-project role assignments.
#[async_trait]
pub trait MembershipRepository: Send + Sync {
    /// Create or update the membership of `user_id` in `project_id`.
    async fn upsert(&self, project_id: Id, user_id: Id, role: Role) -> Result<Membership>;
    /// The membership of a user in a project, if any.
    async fn find(&self, project_id: Id, user_id: Id) -> Result<Option<Membership>>;
    /// Remove a member from a project (admin).
    async fn remove(&self, project_id: Id, user_id: Id) -> Result<()>;
    /// All memberships for a project.
    async fn list_for_project(&self, project_id: Id) -> Result<Vec<Membership>>;
    /// All memberships for a user (across projects).
    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Membership>>;
    /// Members of a project joined with their user records.
    async fn members(&self, project_id: Id) -> Result<Vec<(User, Role)>>;
}
