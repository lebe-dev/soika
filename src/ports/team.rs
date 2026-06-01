//! Team repository port (MVP §4.1).

use crate::domain::{Id, Team, TeamMember, User};
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

    // --- membership (User × Team) ---
    async fn add_member(&self, team_id: Id, user_id: Id) -> Result<TeamMember>;
    async fn remove_member(&self, team_id: Id, user_id: Id) -> Result<()>;
    async fn members(&self, team_id: Id) -> Result<Vec<User>>;
}
