//! User and session domain types.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// How an account authenticates.
///
/// `Local` accounts hold an Argon2 PHC `password_hash`. `Oidc` accounts are
/// provisioned via OAuth/OIDC and carry an empty-string `password_hash`
/// sentinel, so password login is impossible by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthProvider {
    Local,
    Oidc,
}

impl AuthProvider {
    /// TEXT representation stored in the `users.auth_provider` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthProvider::Local => "local",
            AuthProvider::Oidc => "oidc",
        }
    }

    /// Parse the stored TEXT value; unknown values fall back to `Local`.
    pub fn from_db(value: &str) -> Self {
        match value {
            "oidc" => AuthProvider::Oidc,
            _ => AuthProvider::Local,
        }
    }
}

/// Instance-wide role, stored on the `users.instance_role` column.
///
/// Replaces the former `is_admin: bool`. Independent of per-team [`super::Role`]:
/// it grants instance-level capabilities (managing teams, users, invites and —
/// for `Owner` — destructive/critical operations). `Owner` and `Manager` also
/// see and administer every team and project; `Member` only what their team
/// memberships grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstanceRole {
    /// Full control, including granting/revoking `Owner` and destructive ops.
    Owner,
    /// Operational control: teams, projects, users, invites, instance roles
    /// (except `Owner`).
    Manager,
    /// No instance-wide capabilities; access is governed by team membership.
    Member,
}

impl InstanceRole {
    /// TEXT representation stored in the `users.instance_role` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            InstanceRole::Owner => "owner",
            InstanceRole::Manager => "manager",
            InstanceRole::Member => "member",
        }
    }

    /// Parse the stored TEXT value; unknown values fall back to `Member`.
    pub fn from_db(value: &str) -> Self {
        match value {
            "owner" => InstanceRole::Owner,
            "manager" => InstanceRole::Manager,
            _ => InstanceRole::Member,
        }
    }

    /// Whether this role is `Owner`.
    pub fn is_owner(&self) -> bool {
        matches!(self, InstanceRole::Owner)
    }

    /// Whether this role has instance-management capabilities (`Owner | Manager`).
    pub fn can_manage_instance(&self) -> bool {
        matches!(self, InstanceRole::Owner | InstanceRole::Manager)
    }
}

impl FromStr for InstanceRole {
    type Err = crate::error::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "owner" => Ok(InstanceRole::Owner),
            "manager" => Ok(InstanceRole::Manager),
            "member" => Ok(InstanceRole::Member),
            other => Err(crate::error::Error::validation(format!(
                "invalid instance role: {other}"
            ))),
        }
    }
}

/// Account activation status.
///
/// `Active` accounts authenticate normally. `Pending` accounts exist but cannot
/// hold a session: they are created when the OAuth admin-approval flow
/// (`OAUTH_REQUIRE_APPROVAL`) is on and stay locked out until an instance admin
/// approves them. Local/password accounts are always `Active`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Active,
    Pending,
}

impl UserStatus {
    /// TEXT representation stored in the `users.status` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            UserStatus::Active => "active",
            UserStatus::Pending => "pending",
        }
    }

    /// Parse the stored TEXT value; unknown values fall back to `Active`.
    pub fn from_db(value: &str) -> Self {
        match value {
            "pending" => UserStatus::Pending,
            _ => UserStatus::Active,
        }
    }

    /// Whether the account is awaiting admin approval.
    pub fn is_pending(&self) -> bool {
        matches!(self, UserStatus::Pending)
    }
}

/// An authenticated account with credentials and profile settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    /// Argon2 PHC string. Never serialized to clients.
    #[serde(skip_serializing)]
    pub password_hash: String,
    /// Instance-wide role (`Owner | Manager | Member`).
    pub instance_role: InstanceRole,
    /// User-level opt-out from all email notifications.
    pub notifications_enabled: bool,
    /// Origin of the account: local password or OIDC.
    pub auth_provider: AuthProvider,
    /// Activation status; `Pending` accounts await admin approval and hold no session.
    pub status: UserStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A server-side session (opaque, signed cookie). No JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: Id,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}
