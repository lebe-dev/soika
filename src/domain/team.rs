//! Team domain types.

use super::{Id, Timestamp};
use crate::domain::TeamRole;
use serde::{Deserialize, Serialize};

/// A named group of users granting access to its assigned projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Id,
    pub name: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
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
