//! Membership and role domain types.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Per-project role. Roles are scoped per project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Configure project, manage retention/mute, invite & remove members.
    Admin,
    /// View issues/events, resolve/mute issues.
    Member,
}

impl Role {
    /// Stable string form persisted in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = crate::error::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Role::Admin),
            "member" => Ok(Role::Member),
            other => Err(crate::error::Error::validation(format!(
                "invalid role: {other}"
            ))),
        }
    }
}

/// Links a user to a project with a role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Membership {
    pub project_id: Id,
    pub user_id: Id,
    pub role: Role,
    pub created_at: Timestamp,
}
