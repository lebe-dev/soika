//! Team API handlers (MVP §16 Teams, §4.1).
//!
//! This module also hosts the shared session-auth extractor ([`AuthUser`],
//! [`AdminUser`]) and the JSON error wrapper ([`ApiError`]) used by the other
//! internal-API handlers (profile, settings) until they are promoted to a
//! dedicated shared module — see the integration followups.

use axum::async_trait;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::session_id_from_headers;
use crate::domain::{Id, Team, Timestamp, User};
use crate::error::Error;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// JSON error wrapper
// ---------------------------------------------------------------------------

/// Wraps a domain [`Error`] so internal-API handlers can `?`-propagate it and
/// have it rendered as a JSON `{ "error": "..." }` body with the right status.
///
/// The library `Error` type intentionally has no `IntoResponse` (HTTP mapping
/// lives in the API layer, per `error.rs`); this wrapper is that mapping.
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        ApiError(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::Validation(_) => StatusCode::BAD_REQUEST,
            Error::Auth(_) => StatusCode::UNAUTHORIZED,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(ErrorBody {
            error: self.0.to_string(),
        });
        (status, body).into_response()
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

// ---------------------------------------------------------------------------
// Auth extractors
// ---------------------------------------------------------------------------

/// An authenticated user, resolved from the session cookie (MVP §10.1).
///
/// Extraction fails with `401` when no valid, unexpired session is present.
pub struct AuthUser(pub User);

/// An authenticated **instance admin** (built-in admin, §11). Extraction fails
/// with `401` when unauthenticated and `403` when the user is not an admin.
pub struct AdminUser(pub User);

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = authenticate(parts, state).await?;
        Ok(AuthUser(user))
    }
}

#[async_trait]
impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = authenticate(parts, state).await?;
        if !user.is_admin {
            return Err(ApiError(Error::Forbidden("admin access required".into())));
        }
        Ok(AdminUser(user))
    }
}

/// Resolve the current user from the **signed** session cookie, or `Error::Auth`.
///
/// The login handler sets a signed cookie (`<payload>.<sig>`); we must verify
/// the signature and extract the inner opaque session id before looking it up,
/// otherwise the raw signed value never matches a stored session id.
async fn authenticate(parts: &Parts, state: &AppState) -> Result<User, Error> {
    let session_id = session_id_from_headers(&state.config.secret_key, &parts.headers)
        .ok_or_else(|| Error::Auth("no session".into()))?;

    let session = state
        .sessions
        .find(&session_id)
        .await?
        .ok_or_else(|| Error::Auth("invalid or expired session".into()))?;

    state
        .users
        .find_by_id(session.user_id)
        .await?
        .ok_or_else(|| Error::Auth("session user no longer exists".into()))
}

/// Parse a path id segment into a domain [`Id`], mapping a bad UUID to a 400.
fn parse_id(raw: &str) -> Result<Id, Error> {
    Id::parse_str(raw).map_err(|_| Error::validation(format!("invalid id: {raw}")))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// A team plus its members and assigned projects (MVP §4.1, §15 Teams).
#[derive(Debug, Serialize)]
pub struct TeamView {
    pub id: Id,
    pub name: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub members: Vec<MemberView>,
    pub projects: Vec<ProjectView>,
}

/// A team list entry, with member and project counts (cheap list view).
#[derive(Debug, Serialize)]
pub struct TeamSummary {
    pub id: Id,
    pub name: String,
    pub member_count: usize,
    pub project_count: usize,
}

/// A team member, with only client-safe user fields.
#[derive(Debug, Serialize)]
pub struct MemberView {
    pub id: Id,
    pub email: String,
    pub display_name: String,
}

impl From<User> for MemberView {
    fn from(u: User) -> Self {
        MemberView {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
        }
    }
}

/// A project assigned to a team.
#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: Id,
    pub name: String,
    pub slug: String,
}

/// `POST /teams` body.
#[derive(Debug, Deserialize)]
pub struct CreateTeam {
    pub name: String,
}

/// `PATCH /teams/{id}` body.
#[derive(Debug, Deserialize)]
pub struct UpdateTeam {
    pub name: String,
}

/// `POST /teams/{id}/members` body — add a member by user id.
#[derive(Debug, Deserialize)]
pub struct AddMember {
    pub user_id: Id,
}

fn validate_team_name(name: &str) -> Result<String, Error> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::validation("team name must not be empty"));
    }
    Ok(trimmed.to_string())
}

async fn load_team_view(state: &AppState, team: Team) -> Result<TeamView, Error> {
    let members = state.teams.members(team.id).await?;
    let projects = state.projects.list_for_team(team.id).await?;
    Ok(TeamView {
        id: team.id,
        name: team.name,
        created_at: team.created_at,
        updated_at: team.updated_at,
        members: members.into_iter().map(MemberView::from).collect(),
        projects: projects
            .into_iter()
            .map(|p| ProjectView {
                id: p.id,
                name: p.name,
                slug: p.slug,
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /teams` — list teams (any authenticated user). Returns summaries.
pub async fn list(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TeamSummary>>, ApiError> {
    let teams = state.teams.list().await?;
    let mut out = Vec::with_capacity(teams.len());
    for team in teams {
        let members = state.teams.members(team.id).await?;
        let projects = state.projects.list_for_team(team.id).await?;
        out.push(TeamSummary {
            id: team.id,
            name: team.name,
            member_count: members.len(),
            project_count: projects.len(),
        });
    }
    Ok(Json(out))
}

/// `POST /teams` — create a team (instance admin).
pub async fn create(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateTeam>,
) -> Result<(StatusCode, Json<TeamView>), ApiError> {
    let name = validate_team_name(&body.name)?;
    let team = state.teams.create(name).await?;
    let view = load_team_view(&state, team).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// `GET /teams/{id}` — team detail (members, projects).
pub async fn get(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TeamView>, ApiError> {
    let id = parse_id(&id)?;
    let team = state
        .teams
        .find_by_id(id)
        .await?
        .ok_or_else(|| Error::not_found(format!("team {id}")))?;
    let view = load_team_view(&state, team).await?;
    Ok(Json(view))
}

/// `PATCH /teams/{id}` — rename a team (instance admin).
pub async fn update(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateTeam>,
) -> Result<Json<TeamView>, ApiError> {
    let id = parse_id(&id)?;
    let name = validate_team_name(&body.name)?;
    let team = state.teams.rename(id, name).await?;
    let view = load_team_view(&state, team).await?;
    Ok(Json(view))
}

/// `DELETE /teams/{id}` — delete a team (instance admin).
pub async fn delete(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    state.teams.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /teams/{id}/members` — add a member to a team (instance admin).
pub async fn add_member(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AddMember>,
) -> Result<Json<TeamView>, ApiError> {
    let id = parse_id(&id)?;
    // Ensure team & user exist for clear 404s (avoids opaque FK failures).
    let team = state
        .teams
        .find_by_id(id)
        .await?
        .ok_or_else(|| Error::not_found(format!("team {id}")))?;
    if state.users.find_by_id(body.user_id).await?.is_none() {
        return Err(ApiError(Error::not_found(format!("user {}", body.user_id))));
    }
    state.teams.add_member(id, body.user_id).await?;
    let view = load_team_view(&state, team).await?;
    Ok(Json(view))
}

/// `DELETE /teams/{id}/members/{user_id}` — remove a team member (instance admin).
pub async fn remove_member(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let user_id = parse_id(&user_id)?;
    state.teams.remove_member(id, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Session-cookie extraction/verification is covered by `auth::session` tests
    // (signed cookie round-trip + tamper rejection). The API extractor delegates
    // to `auth::session_id_from_headers`, so it is not re-tested here.

    #[test]
    fn validate_team_name_trims_and_rejects_empty() {
        assert_eq!(validate_team_name("  Ops  ").unwrap(), "Ops");
        assert!(validate_team_name("   ").is_err());
        assert!(validate_team_name("").is_err());
    }

    #[test]
    fn parse_id_rejects_garbage() {
        assert!(parse_id("not-a-uuid").is_err());
        let id = Id::new_v4();
        assert_eq!(parse_id(&id.to_string()).unwrap(), id);
    }

    #[test]
    fn api_error_status_mapping() {
        let cases = [
            (Error::not_found("x"), StatusCode::NOT_FOUND),
            (Error::validation("x"), StatusCode::BAD_REQUEST),
            (Error::Auth("x".into()), StatusCode::UNAUTHORIZED),
            (Error::Forbidden("x".into()), StatusCode::FORBIDDEN),
            (Error::Conflict("x".into()), StatusCode::CONFLICT),
            (Error::RateLimited, StatusCode::TOO_MANY_REQUESTS),
            (Error::internal("x"), StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (err, expected) in cases {
            let resp = ApiError(err).into_response();
            assert_eq!(resp.status(), expected);
        }
    }
}
