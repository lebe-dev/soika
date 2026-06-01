//! Invite domain type (MVP §9).

use super::{Id, Role, Timestamp};
use serde::{Deserialize, Serialize};

/// A pending invitation to a project (signed, expiring token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    /// Signed, opaque token; also the lookup key and the `/invite/{token}` path.
    pub token: String,
    pub project_id: Id,
    pub role: Role,
    /// Optional target email (link still works without email — §9).
    pub email: Option<String>,
    /// User id of the inviting admin.
    pub created_by: Option<Id>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    /// Set once the invite has been accepted.
    pub accepted_at: Option<Timestamp>,
}
