//! Passkey (WebAuthn credential) domain type.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// A WebAuthn credential registered by a user.
///
/// `credential` holds the JSON serialization of the verified credential as
/// produced by the WebAuthn library; the domain treats it as an opaque blob so
/// nothing outside `crate::auth::passkey` depends on that representation.
/// `credential_id` is the raw WebAuthn credential id in URL-safe base64 (no
/// padding) and is unique across the instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Passkey {
    pub id: Id,
    pub user_id: Id,
    pub credential_id: String,
    /// Operator-facing label ("MacBook Touch ID").
    pub name: String,
    /// Opaque, library-owned credential serialization.
    pub credential: String,
    pub created_at: Timestamp,
    /// Last successful assertion, `None` until the key is first used to sign in.
    pub last_used_at: Option<Timestamp>,
}

/// Maximum accepted length of a passkey label.
pub const MAX_PASSKEY_NAME_LEN: usize = 64;

/// Normalize and validate a user-supplied passkey label.
///
/// Empty input falls back to `default_name` (the caller passes a sensible one,
/// e.g. "Passkey") so a device that offers no useful hint still yields a label.
pub fn normalize_passkey_name(name: &str, default_name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(default_name.to_string());
    }
    if trimmed.chars().count() > MAX_PASSKEY_NAME_LEN {
        return Err(format!(
            "passkey name must be at most {MAX_PASSKEY_NAME_LEN} characters"
        ));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_name_falls_back_to_default() {
        assert_eq!(normalize_passkey_name("   ", "Passkey").unwrap(), "Passkey");
    }

    #[test]
    fn name_is_trimmed() {
        assert_eq!(
            normalize_passkey_name("  iPhone  ", "Passkey").unwrap(),
            "iPhone"
        );
    }

    #[test]
    fn overlong_name_is_rejected() {
        let long = "x".repeat(MAX_PASSKEY_NAME_LEN + 1);
        assert!(normalize_passkey_name(&long, "Passkey").is_err());
    }
}
