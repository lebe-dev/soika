//! User and session domain types (MVP §10).

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// An authenticated account with credentials and profile settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    /// Argon2 PHC string. Never serialized to clients.
    #[serde(skip_serializing)]
    pub password_hash: String,
    /// Instance-wide admin (built-in admin, §11).
    pub is_admin: bool,
    /// User-level opt-out from all email notifications (§10.3).
    pub notifications_enabled: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A server-side session (opaque, signed cookie — §10.1). No JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: Id,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}
