//! Invite token generation & link building.
//!
//! An invite token is a high-entropy, URL-safe random string that doubles as
//! the DB primary key of the [`crate::domain::Invite`] record. Expiry and the
//! bound team/role are authoritative in the DB row, so a token is only valid
//! while a matching, unexpired, unaccepted invite exists. Tokens are generated
//! with a CSPRNG, making them practically unguessable.

use rand::RngCore;
use rand::rngs::OsRng;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::domain::{Invite, Timestamp};
use crate::error::{Error, Result};

/// Default invite lifetime: 7 days.
pub const INVITE_TTL_DAYS: i64 = 7;

/// Generate a fresh opaque invite token (256 bits of entropy).
pub fn generate_invite_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Build the shareable invite link for a token: `{base_url}/invite/{token}`.
pub fn invite_link(base_url: &str, token: &str) -> String {
    format!("{}/invite/{}", base_url.trim_end_matches('/'), token)
}

/// Validate that an invite is still usable at time `now`: not yet accepted and
/// not expired. Returns `Error::Auth` otherwise so callers can map to 4xx.
pub fn check_invite_usable(invite: &Invite, now: Timestamp) -> Result<()> {
    if invite.accepted_at.is_some() {
        return Err(Error::Auth("invite has already been accepted".into()));
    }
    if invite.expires_at <= now {
        return Err(Error::Auth("invite has expired".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TeamRole;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    fn invite(expires_in: Duration, accepted: bool) -> Invite {
        let now = Utc::now();
        Invite {
            token: generate_invite_token(),
            team_id: Uuid::new_v4(),
            role: TeamRole::Contributor,
            email: None,
            created_by: None,
            created_at: now,
            expires_at: now + expires_in,
            accepted_at: accepted.then_some(now),
        }
    }

    #[test]
    fn tokens_are_unique_and_url_safe() {
        let a = generate_invite_token();
        let b = generate_invite_token();
        assert_ne!(a, b);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn link_joins_base_url_without_double_slash() {
        assert_eq!(
            invite_link("http://localhost:8080/", "tok"),
            "http://localhost:8080/invite/tok"
        );
        assert_eq!(
            invite_link("http://localhost:8080", "tok"),
            "http://localhost:8080/invite/tok"
        );
    }

    #[test]
    fn usable_invite_passes() {
        let inv = invite(Duration::days(1), false);
        assert!(check_invite_usable(&inv, Utc::now()).is_ok());
    }

    #[test]
    fn expired_invite_rejected() {
        let inv = invite(Duration::seconds(-1), false);
        let err = check_invite_usable(&inv, Utc::now()).unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
    }

    #[test]
    fn accepted_invite_rejected() {
        let inv = invite(Duration::days(1), true);
        let err = check_invite_usable(&inv, Utc::now()).unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
    }
}
