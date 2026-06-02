//! Profile API handlers.
//!
//! Lets the current user view and edit their profile: display name, password
//! (verify old → re-hash new), and the user-level `notifications_enabled`
//! email opt-out. Session auth via the shared [`AuthUser`] extractor.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::api::teams::{ApiError, AuthUser};
use crate::auth::{hash_password, verify_password};
use crate::domain::{Id, User};
use crate::error::Error;
use crate::ports::UserUpdate;
use crate::state::AppState;

/// Client-safe view of a user's profile (never includes the password hash).
#[derive(Debug, Serialize)]
pub struct ProfileView {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    pub notifications_enabled: bool,
}

impl From<User> for ProfileView {
    fn from(u: User) -> Self {
        ProfileView {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            is_admin: u.is_admin,
            notifications_enabled: u.notifications_enabled,
        }
    }
}

/// `PATCH /profile` body. All fields optional; absent fields are unchanged.
///
/// A password change requires both `current_password` and `new_password`;
/// the current password is verified before the new one is hashed and stored.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateProfile {
    pub display_name: Option<String>,
    pub notifications_enabled: Option<bool>,
    pub current_password: Option<String>,
    pub new_password: Option<String>,
}

/// Minimum acceptable length for a new password.
const MIN_PASSWORD_LEN: usize = 8;

/// `GET /profile` — current user's profile.
pub async fn get(AuthUser(user): AuthUser) -> Json<ProfileView> {
    Json(ProfileView::from(user))
}

/// `PATCH /profile` — update display name, notifications toggle, and/or password.
pub async fn update(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateProfile>,
) -> Result<Json<ProfileView>, ApiError> {
    let mut update = UserUpdate::default();
    let mut password_changed = false;

    if let Some(name) = body.display_name {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApiError(Error::validation(
                "display name must not be empty",
            )));
        }
        update.display_name = Some(trimmed.to_string());
    }

    if let Some(enabled) = body.notifications_enabled {
        update.notifications_enabled = Some(enabled);
    }

    // Password change: require & verify the current password, then re-hash.
    match (body.current_password, body.new_password) {
        (Some(current), Some(new)) => {
            let hash = build_password_hash(&current, &new, &user.password_hash)?;
            update.password_hash = Some(hash);
            password_changed = true;
        }
        (None, None) => {}
        _ => {
            return Err(ApiError(Error::validation(
                "both current_password and new_password are required to change password",
            )));
        }
    }

    let updated = state.users.update(user.id, update).await?;

    // On password change, invalidate all sessions (forces re-login everywhere).
    if password_changed {
        state.sessions.delete_for_user(user.id).await?;
    }

    Ok(Json(ProfileView::from(updated)))
}

/// Verify the current password and produce the new argon2 hash, after checking
/// the new password against [`validate_new_password`].
fn build_password_hash(current: &str, new: &str, stored_hash: &str) -> Result<String, Error> {
    if !verify_password(current, stored_hash)? {
        return Err(Error::Auth("current password is incorrect".into()));
    }
    validate_new_password(current, new)?;
    hash_password(new)
}

/// Pure validation of a candidate new password (length + must differ). Split out
/// so it can be unit-tested without the argon2 hashing/verification helpers.
fn validate_new_password(current: &str, new: &str) -> Result<(), Error> {
    if new.len() < MIN_PASSWORD_LEN {
        return Err(Error::validation(format!(
            "new password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if new == current {
        return Err(Error::validation(
            "new password must differ from the current one",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // These exercise the pure password validation rules without touching the
    // argon2 hash/verify helpers (whose bodies live in `crate::auth`).

    #[test]
    fn rejects_too_short_new_password() {
        let err = validate_new_password("correct-horse", "short").unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn rejects_unchanged_password() {
        let err = validate_new_password("correct-horse", "correct-horse").unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn accepts_valid_new_password() {
        assert!(validate_new_password("correct-horse", "battery-staple-9").is_ok());
    }

    #[test]
    fn profile_view_omits_password_hash() {
        // The struct simply has no such field; assert the serialized shape.
        let view = ProfileView {
            id: Id::nil(),
            email: "a@b.c".into(),
            display_name: "A".into(),
            is_admin: false,
            notifications_enabled: true,
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("password"));
        assert!(json.contains("notifications_enabled"));
    }
}
