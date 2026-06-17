//! Role domain types.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Computed **effective** access level a user has to a project. **Not stored** —
/// it is the result of resolving the user's instance role and team membership
/// against the project's owning team (see `crate::api::projects::effective_role`).
/// Kept as a distinct type so handlers gated on `require_member` / `require_admin`
/// need not change.
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

/// Role of a user **within a team**, stored on the `team_members` membership.
///
/// `Admin` can manage the team's composition, team roles, projects and project
/// settings; `Contributor` has read/triage access to the team's projects only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamRole {
    /// Manage team membership/roles, projects and project settings.
    Admin,
    /// View issues/events, resolve/mute issues in the team's projects.
    Contributor,
}

impl TeamRole {
    /// Stable string form persisted in the `team_members.role` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            TeamRole::Admin => "admin",
            TeamRole::Contributor => "contributor",
        }
    }

    /// Parse the stored TEXT value; unknown values fall back to `Contributor`.
    pub fn from_db(value: &str) -> Self {
        match value {
            "admin" => TeamRole::Admin,
            _ => TeamRole::Contributor,
        }
    }
}

impl fmt::Display for TeamRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TeamRole {
    type Err = crate::error::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(TeamRole::Admin),
            "contributor" => Ok(TeamRole::Contributor),
            other => Err(crate::error::Error::validation(format!(
                "invalid team role: {other}"
            ))),
        }
    }
}
