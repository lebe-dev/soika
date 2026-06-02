//! Current-user extractor & per-project role gating.
//!
//! [`CurrentUser`] is an axum extractor that resolves the authenticated user
//! from the signed session cookie + the server-side session record. Handlers
//! that require authentication take it as an argument; missing/invalid sessions
//! yield `401`.
//!
//! Authorization is per project: a user holds a [`Role`] (`admin`/`member`) in
//! each project they belong to. The built-in instance admin bypasses
//! per-project checks entirely.

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};

use crate::domain::{Id, Role, User};
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

/// Pure role-gate: does `held` satisfy the `required` role for an action?
///
/// `admin` satisfies any requirement; `member` satisfies only `member`.
pub fn role_satisfies(held: Role, required: Role) -> bool {
    match required {
        Role::Member => true, // any membership (admin or member) suffices
        Role::Admin => held == Role::Admin,
    }
}

/// Authorize `user` for `required` role on `project_id`.
///
/// The instance admin is granted unconditionally. Otherwise the user's
/// membership is loaded and its role checked. Returns the effective role on
/// success; `Error::Forbidden` / `Error::Auth` otherwise.
pub async fn authorize_project(
    state: &AppState,
    user: &User,
    project_id: Id,
    required: Role,
) -> Result<Role> {
    if user.is_admin {
        return Ok(Role::Admin);
    }

    let membership = state
        .memberships
        .find(project_id, user.id)
        .await?
        .ok_or_else(|| Error::Forbidden("not a member of this project".into()))?;

    if !role_satisfies(membership.role, required) {
        return Err(Error::Forbidden(format!(
            "requires {required} role on this project"
        )));
    }
    Ok(membership.role)
}

/// Require that `user` can view a project (any membership or instance admin).
pub async fn require_project_member(state: &AppState, user: &User, project_id: Id) -> Result<Role> {
    authorize_project(state, user, project_id, Role::Member).await
}

/// Require that `user` administers a project (project admin or instance admin).
pub async fn require_project_admin(state: &AppState, user: &User, project_id: Id) -> Result<Role> {
    authorize_project(state, user, project_id, Role::Admin).await
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
    fn admin_satisfies_every_requirement() {
        assert!(role_satisfies(Role::Admin, Role::Admin));
        assert!(role_satisfies(Role::Admin, Role::Member));
    }

    #[test]
    fn member_satisfies_only_member() {
        assert!(role_satisfies(Role::Member, Role::Member));
        assert!(!role_satisfies(Role::Member, Role::Admin));
    }

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
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
