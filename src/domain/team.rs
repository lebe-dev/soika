//! Team domain types.

use super::{Id, Timestamp};
use crate::domain::TeamRole;
use serde::{Deserialize, Serialize};

/// A named group of users granting access to its assigned projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Id,
    /// Short public id (10 lowercase hex chars) assigned by the database; used
    /// in web URLs and accepted by the team API instead of the verbose UUID,
    /// which remains the internal key.
    pub short_id: String,
    pub name: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub orphaned_at: Option<Timestamp>,
}

/// Membership link of a user in a team (User × Team), carrying the team role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub team_id: Id,
    pub user_id: Id,
    /// Role of this user within the team (`Admin | Contributor`).
    pub role: TeamRole,
    pub created_at: Timestamp,
}
