//! User and session domain types.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

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

/// An authenticated account with credentials and profile settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    /// Argon2 PHC string. Never serialized to clients.
    #[serde(skip_serializing)]
    pub password_hash: String,
    /// Instance-wide admin (built-in admin).
    pub is_admin: bool,
    /// User-level opt-out from all email notifications.
    pub notifications_enabled: bool,
    /// Origin of the account: local password or OIDC.
    pub auth_provider: AuthProvider,
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
