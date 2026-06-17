//! Team API handlers (Teams).
//!
//! This module also hosts the shared session-auth extractor ([`AuthUser`],
//! [`AdminUser`]) and the JSON error wrapper ([`ApiError`]) used by the other
//! internal-API handlers (profile, settings) until they are promoted to a
//! dedicated shared module — see the integration followups.

use axum::Json;
use axum::async_trait;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::auth::session_id_from_headers;
use crate::domain::{Id, Team, TeamRole, Timestamp, User};
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
        // 5xx faults carry their cause only in the response body, which never
        // reaches the operator. Log it so the real reason (e.g. the SMTP error
        // behind a failed test-email) surfaces in tracing and Sentry instead of
        // tower-http's contentless "response failed".
        if status.is_server_error() {
            tracing::error!(error = %self.0, "request failed");
        }
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

/// An authenticated user, resolved from the session cookie.
///
/// Extraction fails with `401` when no valid, unexpired session is present.
pub struct AuthUser(pub User);

/// An authenticated **instance manager** (`Owner | Manager`). Extraction fails
/// with `401` when unauthenticated and `403` when the user cannot manage the
/// instance. Owner-only operations re-check [`User::instance_role`] inline.
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
        if !user.instance_role.can_manage_instance() {
            return Err(ApiError(Error::Forbidden(
                "requires instance manager or owner".into(),
            )));
        }
        Ok(AdminUser(user))
    }
}

/// An optionally-authenticated user: `Some` when a valid session is present,
/// `None` otherwise. Extraction never fails, so handlers that serve both
/// signed-in and anonymous callers (e.g. the bootstrap `/auth/config`) can
/// branch on the session instead of being gated behind a `401`.
pub struct OptionalAuthUser(pub Option<User>);

#[async_trait]
impl FromRequestParts<AppState> for OptionalAuthUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Any failure collapses to "anonymous" — this is a UI hint, not an
        // authorization boundary. But `Error::Auth` (no cookie / expired session
        // / removed user) is the *expected* anonymous case, whereas anything else
        // (e.g. a transient session/user store error) is a real problem we must
        // not bury silently. Log the latter before still degrading to `None`.
        match authenticate(parts, state).await {
            Ok(user) => Ok(OptionalAuthUser(Some(user))),
            Err(Error::Auth(_)) => Ok(OptionalAuthUser(None)),
            Err(e) => {
                tracing::warn!(error = %e, "optional auth: session lookup failed, treating as anonymous");
                Ok(OptionalAuthUser(None))
            }
        }
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

/// A team plus its members and assigned projects (Teams).
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

/// A team member, with client-safe user fields plus the team role.
#[derive(Debug, Serialize)]
pub struct MemberView {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    /// Role of this user within the team (`Admin | Contributor`).
    pub role: TeamRole,
}

impl MemberView {
    fn from_member(u: User, role: TeamRole) -> Self {
        MemberView {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            role,
        }
    }
}

/// A project assigned to a team.
#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: String,
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

/// `POST /teams/{id}/members` body — add a member by user id with a team role.
#[derive(Debug, Deserialize)]
pub struct AddMember {
    pub user_id: Id,
    /// Team role to grant. Defaults to `Contributor` when omitted.
    #[serde(default)]
    pub role: Option<TeamRole>,
}

/// `PATCH /teams/{id}/members/{user_id}` body — change a member's team role.
#[derive(Debug, Deserialize)]
pub struct SetMemberRole {
    pub role: TeamRole,
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
        members: members
            .into_iter()
            .map(|(u, role)| MemberView::from_member(u, role))
            .collect(),
        projects: projects
            .into_iter()
            .map(|p| ProjectView {
                id: p.short_id,
                name: p.name,
                slug: p.slug,
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Team summaries (id, name, member/project counts) visible to `user`.
///
/// Instance `Owner | Manager` see every team; other users see only the teams
/// they belong to. Shared by `GET /teams` and the `/auth/config` dashboard bootstrap.
pub async fn summaries(state: &AppState, user: &User) -> Result<Vec<TeamSummary>, ApiError> {
    let teams = if user.instance_role.can_manage_instance() {
        state.teams.list().await?
    } else {
        state.teams.list_for_user(user.id).await?
    };
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
    Ok(out)
}

/// `GET /teams` — list teams visible to the caller. Returns summaries.
///
/// Instance `Owner | Manager` see all teams; other users see only their own.
pub async fn list(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TeamSummary>>, ApiError> {
    Ok(Json(summaries(&state, &user).await?))
}

/// `POST /teams` — create a team (instance manager/owner).
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
///
/// Visible to instance `Owner | Manager` and to members of the team; other
/// users get a `403`, consistent with the project access rules.
pub async fn get(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TeamView>, ApiError> {
    let id = parse_id(&id)?;
    let team = state
        .teams
        .find_by_id(id)
        .await?
        .ok_or_else(|| Error::not_found(format!("team {id}")))?;
    if !user.instance_role.can_manage_instance() && !state.teams.is_member(id, user.id).await? {
        return Err(ApiError(Error::Forbidden(
            "you do not have access to this team".into(),
        )));
    }
    let view = load_team_view(&state, team).await?;
    Ok(Json(view))
}

/// `PATCH /teams/{id}` — rename a team (instance manager/owner).
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

/// `DELETE /teams/{id}` — delete a team (instance manager/owner).
pub async fn delete(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    state.teams.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Require that `user` may manage `team_id`'s membership: either an instance
/// `Owner | Manager`, or a `TeamRole::Admin` of that specific team.
async fn require_team_manager(state: &AppState, user: &User, team_id: Id) -> Result<(), ApiError> {
    if user.instance_role.can_manage_instance() {
        return Ok(());
    }
    if state.teams.member_role(team_id, user.id).await? == Some(TeamRole::Admin) {
        return Ok(());
    }
    Err(ApiError(Error::Forbidden(
        "requires team admin (or instance manager/owner)".into(),
    )))
}

/// `POST /teams/{id}/members` — add a member to a team.
///
/// Gated by an instance `Owner | Manager` OR a `TeamRole::Admin` of this team.
pub async fn add_member(
    AuthUser(caller): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AddMember>,
) -> Result<Json<TeamView>, ApiError> {
    let id = parse_id(&id)?;
    require_team_manager(&state, &caller, id).await?;
    // Ensure team & user exist for clear 404s (avoids opaque FK failures).
    let team = state
        .teams
        .find_by_id(id)
        .await?
        .ok_or_else(|| Error::not_found(format!("team {id}")))?;
    if state.users.find_by_id(body.user_id).await?.is_none() {
        return Err(ApiError(Error::not_found(format!("user {}", body.user_id))));
    }
    let role = body.role.unwrap_or(TeamRole::Contributor);
    state.teams.add_member(id, body.user_id, role).await?;
    let view = load_team_view(&state, team).await?;
    Ok(Json(view))
}

/// `PATCH /teams/{id}/members/{user_id}` — change a member's team role.
///
/// Gated by an instance `Owner | Manager` OR a `TeamRole::Admin` of this team.
/// Refuses to demote the team's last `Admin` so a team always retains one.
pub async fn set_member_role(
    AuthUser(caller): AuthUser,
    State(state): State<AppState>,
    Path((id, user_id)): Path<(String, String)>,
    Json(body): Json<SetMemberRole>,
) -> Result<Json<TeamView>, ApiError> {
    let id = parse_id(&id)?;
    let user_id = parse_id(&user_id)?;
    require_team_manager(&state, &caller, id).await?;

    let team = state
        .teams
        .find_by_id(id)
        .await?
        .ok_or_else(|| Error::not_found(format!("team {id}")))?;

    let members = state.teams.members(id).await?;
    let Some((_, current_role)) = members.iter().find(|(u, _)| u.id == user_id) else {
        return Err(ApiError(Error::not_found("member not found in team")));
    };

    // Prevent orphaning the team: refuse to demote the last admin.
    if *current_role == TeamRole::Admin && body.role != TeamRole::Admin {
        let admin_count = members
            .iter()
            .filter(|(_, r)| *r == TeamRole::Admin)
            .count();
        if admin_count <= 1 {
            return Err(ApiError(Error::Conflict(
                "cannot demote the last admin of the team".into(),
            )));
        }
    }

    state.teams.set_member_role(id, user_id, body.role).await?;
    let view = load_team_view(&state, team).await?;
    Ok(Json(view))
}

/// `DELETE /teams/{id}/members/{user_id}` — remove a team member.
///
/// Gated by an instance `Owner | Manager` OR a `TeamRole::Admin` of this team.
/// Refuses to remove the team's last `Admin`.
pub async fn remove_member(
    AuthUser(caller): AuthUser,
    State(state): State<AppState>,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let user_id = parse_id(&user_id)?;
    require_team_manager(&state, &caller, id).await?;

    let members = state.teams.members(id).await?;
    if let Some((_, role)) = members.iter().find(|(u, _)| u.id == user_id)
        && *role == TeamRole::Admin
    {
        let admin_count = members
            .iter()
            .filter(|(_, r)| *r == TeamRole::Admin)
            .count();
        if admin_count <= 1 {
            return Err(ApiError(Error::Conflict(
                "cannot remove the last admin of the team".into(),
            )));
        }
    }

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

    /// A `tracing` layer that records the level of every event it sees, so the
    /// test can assert which `into_response` calls emit an `ERROR`.
    #[derive(Clone, Default)]
    struct LevelCapture(std::sync::Arc<std::sync::Mutex<Vec<tracing::Level>>>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LevelCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.0.lock().unwrap().push(*event.metadata().level());
        }
    }

    #[test]
    fn server_errors_log_but_client_errors_do_not() {
        use tracing_subscriber::layer::SubscriberExt;

        let capture = LevelCapture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        tracing::subscriber::with_default(subscriber, || {
            // 4xx: surfaced to the caller, not an operator-facing fault — no log.
            let _ = ApiError(Error::validation("bad input")).into_response();
            // 5xx: the cause lives only in the body, so it must be logged.
            let _ = ApiError(Error::internal("boom")).into_response();
        });

        let levels = capture.0.lock().unwrap();
        assert_eq!(
            *levels,
            vec![tracing::Level::ERROR],
            "exactly the 5xx should emit one ERROR event"
        );
    }
}
