//! Team repository port.

use crate::domain::{Id, Team, TeamMember, TeamRole, User};
use crate::error::Result;
use async_trait::async_trait;

/// CRUD for teams plus team membership management.
#[async_trait]
pub trait TeamRepository: Send + Sync {
    async fn create(&self, name: String) -> Result<Team>;
    async fn find_by_id(&self, id: Id) -> Result<Option<Team>>;
    async fn list(&self) -> Result<Vec<Team>>;
    async fn rename(&self, id: Id, name: String) -> Result<Team>;
    async fn delete(&self, id: Id) -> Result<()>;

    /// Teams the given user is a member of.
    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Team>>;

    /// Whether `user_id` is a member of `team_id`.
    async fn is_member(&self, team_id: Id, user_id: Id) -> Result<bool>;

    // --- membership (User × Team × TeamRole) ---
    /// Add (or idempotently re-add) `user_id` to `team_id` with the given role.
    async fn add_member(&self, team_id: Id, user_id: Id, role: TeamRole) -> Result<TeamMember>;
    /// Change the team role of an existing member.
    async fn set_member_role(&self, team_id: Id, user_id: Id, role: TeamRole)
    -> Result<TeamMember>;
    async fn remove_member(&self, team_id: Id, user_id: Id) -> Result<()>;
    /// The team role of `user_id` in `team_id`, if a member.
    async fn member_role(&self, team_id: Id, user_id: Id) -> Result<Option<TeamRole>>;
    /// Members of a team joined with their user records and team role.
    async fn members(&self, team_id: Id) -> Result<Vec<(User, TeamRole)>>;
}
