//! Current-user extractor & the instance-admin gate.
//!
//! [`CurrentUser`] is an axum extractor that resolves the authenticated user
//! from the signed session cookie + the server-side session record. Handlers
//! that require authentication take it as an argument; missing/invalid sessions
//! yield `401`.
//!
//! Per-*project* authorization is **not** decided here: the single source of
//! that rule is [`crate::api::projects::effective_role`] (team-aware). This
//! module only provides authentication ([`CurrentUser`]) and the instance-wide
//! admin gate ([`require_instance_admin`]).

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};

use crate::domain::{Id, User};
use crate::error::{Error, Result};
use crate::state::AppState;

use super::session::session_id_from_headers;

/// The authenticated user for the current request.
///
/// Resolved from the session cookie; wraps the full [`User`] record so handlers
/// can read `is_admin`, `id`, etc. without a second lookup.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub User);

impl CurrentUser {
    /// The user's id.
    pub fn id(&self) -> Id {
        self.0.id
    }

    /// Whether this user is the instance-wide built-in admin.
    pub fn is_instance_admin(&self) -> bool {
        self.0.is_admin
    }
}

impl std::ops::Deref for CurrentUser {
    type Target = User;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Rejection returned when authentication fails — renders as `401 Unauthorized`.
#[derive(Debug)]
pub struct AuthRejection(pub String);

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, self.0).into_response()
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AuthRejection;

    // Note: the crate-wide `Result` alias is single-arg; spell out the two-arg
    // std `Result` here to match the trait signature.
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> std::result::Result<Self, AuthRejection> {
        let session_id = session_id_from_headers(&state.config.secret_key, &parts.headers)
            .ok_or_else(|| AuthRejection("missing or invalid session".into()))?;

        let session = state
            .sessions
            .find(&session_id)
            .await
            .map_err(|_| AuthRejection("session lookup failed".into()))?
            .ok_or_else(|| AuthRejection("session not found or expired".into()))?;

        let user = state
            .users
            .find_by_id(session.user_id)
            .await
            .map_err(|_| AuthRejection("user lookup failed".into()))?
            .ok_or_else(|| AuthRejection("user no longer exists".into()))?;

        Ok(CurrentUser(user))
    }
}

/// Require that `user` is the instance-wide built-in admin.
pub fn require_instance_admin(user: &User) -> Result<()> {
    if user.is_admin {
        return Ok(());
    }
    Err(Error::Forbidden("requires instance admin".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_admin_gate() {
        let admin = user(true);
        let plain = user(false);
        assert!(require_instance_admin(&admin).is_ok());
        assert!(matches!(
            require_instance_admin(&plain).unwrap_err(),
            Error::Forbidden(_)
        ));
    }

    fn user(is_admin: bool) -> User {
        use chrono::Utc;
        use uuid::Uuid;
        User {
            id: Uuid::new_v4(),
            email: "u@example.com".into(),
            display_name: "U".into(),
            password_hash: String::new(),
            is_admin,
            notifications_enabled: true,
            auth_provider: crate::domain::AuthProvider::Local,
            status: crate::domain::UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
