//! Service settings domain type.

use super::Timestamp;
use serde::{Deserialize, Serialize};

/// Instance-wide settings, managed by the built-in/instance admin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSettings {
    /// Enable/disable public self-registration (mirrors `ALLOW_SIGNUP`).
    pub allow_signup: bool,
    /// Organization display name (`ORGANIZATION_NAME`).
    pub org_name: String,
    pub updated_at: Timestamp,
}
