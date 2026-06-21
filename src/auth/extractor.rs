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
//! role gates ([`require_manager`], [`require_owner`]).

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts, OptionalFromRequestParts};
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
/// can read `instance_role`, `id`, etc. without a second lookup.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub User);

impl CurrentUser {
    /// The user's id.
    pub fn id(&self) -> Id {
        self.0.id
    }

    /// Whether this user has instance-management capabilities (`Owner | Manager`).
    pub fn can_manage_instance(&self) -> bool {
        self.0.instance_role.can_manage_instance()
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

impl CurrentUser {
    // Note: the crate-wide `Result` alias is single-arg; spell out the two-arg
    // std `Result` here to match the trait signatures.
    async fn resolve(
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

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> std::result::Result<Self, AuthRejection> {
        Self::resolve(parts, state).await
    }
}

// axum 0.8 no longer derives `Option<T>` from `FromRequestParts`; an optional
// extractor must opt in via `OptionalFromRequestParts`. A missing or invalid
// session yields `None` (anonymous), never a rejection.
impl OptionalFromRequestParts<AppState> for CurrentUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> std::result::Result<Option<Self>, AuthRejection> {
        Ok(Self::resolve(parts, state).await.ok())
    }
}

/// Optional peer socket address of the request.
///
/// `Some` when the server is started with `into_make_service_with_connect_info`
/// (production), `None` when handlers are driven directly (tests via `oneshot`).
/// axum 0.8 dropped the blanket `Option<ConnectInfo<_>>` extractor, so we read
/// the `ConnectInfo` request extension ourselves and never reject.
#[derive(Debug, Clone, Copy)]
pub struct PeerAddr(pub Option<SocketAddr>);

impl<S: Send + Sync> FromRequestParts<S> for PeerAddr {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Infallible> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0),
        ))
    }
}

/// Require that `user` can manage the instance (`Owner | Manager`).
pub fn require_manager(user: &User) -> Result<()> {
    if user.instance_role.can_manage_instance() {
        return Ok(());
    }
    Err(Error::Forbidden(
        "requires instance manager or owner".into(),
    ))
}

/// Require that `user` is an instance `Owner`.
pub fn require_owner(user: &User) -> Result<()> {
    if user.instance_role.is_owner() {
        return Ok(());
    }
    Err(Error::Forbidden("requires instance owner".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::InstanceRole;

    #[test]
    fn manager_gate_allows_owner_and_manager() {
        assert!(require_manager(&user(InstanceRole::Owner)).is_ok());
        assert!(require_manager(&user(InstanceRole::Manager)).is_ok());
        assert!(matches!(
            require_manager(&user(InstanceRole::Member)).unwrap_err(),
            Error::Forbidden(_)
        ));
    }

    #[test]
    fn owner_gate_allows_only_owner() {
        assert!(require_owner(&user(InstanceRole::Owner)).is_ok());
        assert!(matches!(
            require_owner(&user(InstanceRole::Manager)).unwrap_err(),
            Error::Forbidden(_)
        ));
        assert!(matches!(
            require_owner(&user(InstanceRole::Member)).unwrap_err(),
            Error::Forbidden(_)
        ));
    }

    fn user(instance_role: InstanceRole) -> User {
        use chrono::Utc;
        use uuid::Uuid;
        User {
            id: Uuid::new_v4(),
            email: "u@example.com".into(),
            display_name: "U".into(),
            password_hash: String::new(),
            instance_role,
            notifications_enabled: true,
            auth_provider: crate::domain::AuthProvider::Local,
            status: crate::domain::UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
