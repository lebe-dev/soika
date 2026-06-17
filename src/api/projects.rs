//! Project API handlers (Projects).
//!
//! Session-cookie auth (see [`CurrentUser`]); per-project role checks.
//! This module also hosts the shared API helpers (`CurrentUser`, JSON error
//! mapping, role guards, instance gates) re-used by the sibling API modules
//! (`issues`, `events`, `invites`).

// Auth/error guards return `Result<T, Response>`; axum's `Response` is large,
// which is fine here (these are control-flow short-circuits, never hot paths).
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::domain::{Id, Project, Role, TeamRole, User};
use crate::error::Error;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Shared helpers (auth, error mapping, role guards) — used across the API mods.
// ---------------------------------------------------------------------------

/// Authenticated user extracted from the session cookie.
///
/// Resolves the opaque session id by verifying the signed `soika_session` cookie
/// via [`crate::auth::session_id_from_headers`], looks up the session (expired
/// sessions are not returned by the repo), then loads the user.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub User);

#[axum::async_trait]
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> std::result::Result<Self, Self::Rejection> {
        // The login handler sets a SIGNED cookie (`<payload>.<sig>`); verify the
        // signature and extract the inner opaque session id before lookup, or the
        // raw signed value will never match a stored session id.
        let session_id =
            crate::auth::session_id_from_headers(&state.config.secret_key, &parts.headers)
                .ok_or_else(auth_rejection)?;

        let session = state
            .sessions
            .find(&session_id)
            .await
            .map_err(error_response)?
            .ok_or_else(auth_rejection)?;

        let user = state
            .users
            .find_by_id(session.user_id)
            .await
            .map_err(error_response)?
            .ok_or_else(auth_rejection)?;

        Ok(CurrentUser(user))
    }
}

/// Standard `401` JSON rejection used by the auth extractor.
fn auth_rejection() -> Response {
    json_error(StatusCode::UNAUTHORIZED, "authentication required")
}

/// Shape of every JSON error body returned by the API.
#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

/// Build a JSON error response with the given status and message.
pub fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}

/// Whether `value` is a well-formed http(s) URL (webhook channel).
///
/// The API is the real trust boundary (the browser's `type="url"` hint does not
/// cover programmatic callers), so a malformed URL is rejected at save time
/// rather than being persisted and then silently failing on every later
/// delivery attempt — turning a typo into a permanent, log-only failure.
fn is_valid_webhook_url(value: &str) -> bool {
    match reqwest::Url::parse(value) {
        Ok(url) => matches!(url.scheme(), "http" | "https"),
        Err(_) => false,
    }
}

/// Map a [`crate::error::Error`] to an HTTP JSON response (error mapping).
pub fn error_response(err: Error) -> Response {
    let status = match &err {
        Error::NotFound(_) => StatusCode::NOT_FOUND,
        Error::Validation(_) | Error::Serde(_) => StatusCode::BAD_REQUEST,
        Error::Auth(_) => StatusCode::UNAUTHORIZED,
        Error::Forbidden(_) => StatusCode::FORBIDDEN,
        Error::Conflict(_) => StatusCode::CONFLICT,
        Error::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        Error::Db(_) | Error::Mail(_) | Error::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_error(status, err.to_string())
}

/// Parse a path id into a [`Id`] (UUID), mapping failure to a `400`.
///
/// Used for ids that are still UUIDs on the wire (user ids, event ids). Project
/// and issue path params are short public ids — resolve those with
/// [`resolve_project`] / `issues::authorize_issue` instead.
pub fn parse_id(raw: &str) -> std::result::Result<Id, Response> {
    Id::parse_str(raw).map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid id"))
}

/// Resolve a project from its short public id (the web URL / SPA path param),
/// returning the full project or a `404` JSON response.
///
/// The internal UUID lives on [`Project::id`]; handlers use that for membership
/// checks and downstream queries while the wire only ever speaks short ids.
pub async fn resolve_project(
    state: &AppState,
    short_id: &str,
) -> std::result::Result<Project, Response> {
    state
        .projects
        .find_by_short_id(short_id)
        .await
        .map_err(error_response)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "project not found"))
}

/// Resolve the caller's effective role on a project (Variant A, closed membership).
///
/// This is **the** project-authorization rule for the whole codebase: project
/// access derives solely from instance role and team membership of the owning
/// team — there is no direct project membership. New handlers MUST gate project
/// access through this (via [`require_member`] / [`require_admin`]).
///
/// Resolution order:
/// 1. Instance `Owner | Manager` ⇒ `Some(Role::Admin)` for every project.
/// 2. Otherwise the caller's team role on the project's owning team:
///    - `TeamRole::Admin`       ⇒ `Some(Role::Admin)`,
///    - `TeamRole::Contributor` ⇒ `Some(Role::Member)`.
/// 3. Otherwise `None` (no access — closed membership).
pub async fn effective_role(
    state: &AppState,
    user: &User,
    project_id: Id,
) -> std::result::Result<Option<Role>, Response> {
    if user.instance_role.can_manage_instance() {
        return Ok(Some(Role::Admin));
    }

    // Access is granted only by membership in the project's owning team.
    let Some(project) = state
        .projects
        .find_by_id(project_id)
        .await
        .map_err(error_response)?
    else {
        return Ok(None);
    };

    let team_role = state
        .teams
        .member_role(project.team_id, user.id)
        .await
        .map_err(error_response)?;

    Ok(resolve_role(user.instance_role, team_role))
}

/// Pure resolver implementing the Variant-A effective-role truth table:
/// instance `Owner | Manager` ⇒ `Admin`; otherwise the team role maps
/// `Admin ⇒ Admin`, `Contributor ⇒ Member`; no team membership ⇒ `None`.
///
/// Split out so the matrix can be unit-tested without DB/`AppState` wiring;
/// [`effective_role`] is the async wrapper that resolves the team role.
fn resolve_role(
    instance_role: crate::domain::InstanceRole,
    team_role: Option<TeamRole>,
) -> Option<Role> {
    if instance_role.can_manage_instance() {
        return Some(Role::Admin);
    }
    team_role.map(|role| match role {
        TeamRole::Admin => Role::Admin,
        TeamRole::Contributor => Role::Member,
    })
}

/// Require that the caller can view the project (any role). Returns the role.
pub async fn require_member(
    state: &AppState,
    user: &User,
    project_id: Id,
) -> std::result::Result<Role, Response> {
    match effective_role(state, user, project_id).await? {
        Some(role) => Ok(role),
        None => Err(json_error(
            StatusCode::FORBIDDEN,
            "you do not have access to this project",
        )),
    }
}

/// Require that the caller is an admin of the project (admin actions).
pub async fn require_admin(
    state: &AppState,
    user: &User,
    project_id: Id,
) -> std::result::Result<(), Response> {
    match require_member(state, user, project_id).await? {
        Role::Admin => Ok(()),
        Role::Member => Err(json_error(
            StatusCode::FORBIDDEN,
            "admin role required for this action",
        )),
    }
}

/// Require that the caller can manage the instance (`Owner | Manager`).
///
/// Instance gate (not project-scoped): used by team CRUD, user management and
/// invite endpoints. Returns a `403` JSON response otherwise.
pub fn require_manager(user: &User) -> std::result::Result<(), Response> {
    if user.instance_role.can_manage_instance() {
        return Ok(());
    }
    Err(json_error(
        StatusCode::FORBIDDEN,
        "requires instance manager or owner",
    ))
}

/// Require that the caller is an instance `Owner`.
///
/// Instance gate for destructive/critical operations (granting/revoking `Owner`,
/// deleting an `Owner`). Returns a `403` JSON response otherwise.
pub fn require_owner(user: &User) -> std::result::Result<(), Response> {
    if user.instance_role.is_owner() {
        return Ok(());
    }
    Err(json_error(StatusCode::FORBIDDEN, "requires instance owner"))
}

/// Build the full DSN string for a project from its public key.
///
/// Format mirrors Sentry: `{scheme}://{public_key}@{host}[:port]/{project_id}`.
pub fn build_dsn(base_url: &str, public_key: &str, project_id: Id) -> String {
    // Split scheme from the rest of base_url so we can inject the public key.
    let (scheme, rest) = match base_url.split_once("://") {
        Some((s, r)) => (s, r),
        None => ("http", base_url),
    };
    let host = rest.trim_end_matches('/');
    format!("{scheme}://{public_key}@{host}/{project_id}")
}

/// Build the two ingestion endpoint URLs for a project: the modern
/// `/envelope/` and the legacy `/store/` (both under `base_url`). Returned as
/// `(envelope_url, store_url)` for the raw-HTTP setup snippet.
fn ingest_urls(base_url: &str, project_id: Id) -> (String, String) {
    let base = base_url.trim_end_matches('/');
    (
        format!("{base}/api/{project_id}/envelope/"),
        format!("{base}/api/{project_id}/store/"),
    )
}

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

/// Project as returned to the SvelteKit client (includes derived DSN).
#[derive(Debug, Serialize)]
pub struct ProjectView {
    /// Short public id (used in web URLs and as the SPA-facing path param). The
    /// internal UUID is never exposed to the client.
    pub id: String,
    pub team_id: Id,
    pub name: String,
    pub slug: String,
    pub dsn_public_key: String,
    pub dsn: String,
    pub retention_events: i64,
    /// Age-based retention in days; 0 disables age-based pruning.
    pub retention_days: i64,
    pub muted: bool,
    /// Optional per-project webhook URL for notifications.
    pub webhook_url: Option<String>,
    pub created_at: crate::domain::Timestamp,
    pub updated_at: crate::domain::Timestamp,
}

impl ProjectView {
    fn from_project(project: Project, base_url: &str) -> Self {
        let dsn = build_dsn(base_url, &project.dsn_public_key, project.id);
        ProjectView {
            id: project.short_id,
            team_id: project.team_id,
            name: project.name,
            slug: project.slug,
            dsn_public_key: project.dsn_public_key,
            dsn,
            retention_events: project.retention_events,
            retention_days: project.retention_days,
            muted: project.muted,
            webhook_url: project.webhook_url,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

/// Project detail returned by `GET /projects/{id}`: the project plus its
/// default (unresolved) issue list, so the project page boots without a
/// follow-up `/projects/{id}/issues` request for the initial view.
///
/// `project` is flattened so the JSON shape stays a superset of [`ProjectView`]
/// (the list/create/update responses) with one extra `issues` field.
#[derive(Debug, Serialize)]
pub struct ProjectDetailView {
    #[serde(flatten)]
    pub project: ProjectView,
    pub issues: Vec<crate::api::issues::IssueView>,
}

/// A project plus its unresolved-issue count, for the dashboard bootstrap.
///
/// `project` is flattened so the JSON shape is a superset of [`ProjectView`]
/// with one extra `unresolved_count` field.
#[derive(Debug, Serialize)]
pub struct ProjectOverviewView {
    #[serde(flatten)]
    pub project: ProjectView,
    pub unresolved_count: i64,
    pub favorited: bool,
}

/// Projects visible to `user`, each with its unresolved-issue count and favorite flag.
///
/// Mirrors [`list`]'s visibility rule (instance `Owner | Manager` see all
/// projects; others only their team's), then fetches every project's open-issue count in a
/// single batched query. Used by the `/auth/config` bootstrap so the dashboard
/// renders from one request instead of fanning out to `/projects`, `/teams`
/// and one `/issues` call per project. Results are sorted: favorited projects
/// first, then alphabetically by name.
pub async fn overviews(
    state: &AppState,
    user: &User,
) -> crate::error::Result<Vec<ProjectOverviewView>> {
    let projects = if user.instance_role.can_manage_instance() {
        state.projects.list().await?
    } else {
        state.projects.list_for_user(user.id).await?
    };

    let ids: Vec<Id> = projects.iter().map(|p| p.id).collect();
    let counts = state.issues.unresolved_counts(&ids).await?;
    let favorite_ids = state.favorites.list_for_user(user.id).await?;
    let favorite_set: std::collections::HashSet<Id> = favorite_ids.into_iter().collect();

    let mut views: Vec<ProjectOverviewView> = projects
        .into_iter()
        .map(|project| {
            let unresolved_count = counts.get(&project.id).copied().unwrap_or(0);
            let favorited = favorite_set.contains(&project.id);
            ProjectOverviewView {
                project: ProjectView::from_project(project, &state.config.base_url),
                unresolved_count,
                favorited,
            }
        })
        .collect();

    views.sort_by(|a, b| {
        b.favorited
            .cmp(&a.favorited)
            .then_with(|| a.project.name.cmp(&b.project.name))
    });

    Ok(views)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /projects` — list projects visible to the current user.
///
/// Instance `Owner | Manager` see all projects; other users see only projects
/// owned by a team they belong to.
pub async fn list(State(state): State<AppState>, CurrentUser(user): CurrentUser) -> Response {
    let projects = if user.instance_role.can_manage_instance() {
        state.projects.list().await
    } else {
        state.projects.list_for_user(user.id).await
    };

    match projects {
        Ok(projects) => {
            let views: Vec<ProjectView> = projects
                .into_iter()
                .map(|p| ProjectView::from_project(p, &state.config.base_url))
                .collect();
            Json(views).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// Request body for creating a project.
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub team_id: Id,
    /// Optional explicit slug; derived from `name` when omitted.
    #[serde(default)]
    pub slug: Option<String>,
    /// Optional retention override; defaults to `DEFAULT_EVENTS_RETENTION`.
    #[serde(default)]
    pub retention_events: Option<i64>,
    /// Optional age-based retention in days; 0/omitted disables.
    #[serde(default)]
    pub retention_days: Option<i64>,
    /// Optional per-project webhook URL for notifications.
    #[serde(default)]
    pub webhook_url: Option<String>,
}

/// `POST /projects` — create a project.
///
/// The caller must be an instance `Owner | Manager`, or a `TeamRole::Admin` of
/// the target team (Variant A: project rights derive from the owning team).
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<CreateProjectRequest>,
) -> Response {
    let name = req.name.trim();
    if name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "name must not be empty");
    }

    // Instance managers/owners may create in any team; otherwise the caller must
    // be a Team Admin of the target team.
    if !user.instance_role.can_manage_instance() {
        match state.teams.member_role(req.team_id, user.id).await {
            Ok(Some(TeamRole::Admin)) => {}
            Ok(_) => {
                return json_error(
                    StatusCode::FORBIDDEN,
                    "you must be an admin of the target team to create a project",
                );
            }
            Err(err) => return error_response(err),
        }
    }

    let slug = match req.slug {
        Some(s) if !s.trim().is_empty() => slugify(s.trim()),
        _ => slugify(name),
    };

    let retention_events = req
        .retention_events
        .filter(|r| *r > 0)
        .unwrap_or(state.config.default_events_retention);

    // Age-based retention is opt-in; non-positive (or omitted) means disabled (0).
    let retention_days = req.retention_days.filter(|d| *d > 0).unwrap_or(0);

    // Normalize + validate the optional webhook URL: a blank/whitespace value is
    // treated as "unset"; a present value must be a valid http(s) URL.
    let webhook_url = match req.webhook_url.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(url) if is_valid_webhook_url(url) => Some(url.to_string()),
        Some(_) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "webhook_url must be a valid http(s) URL",
            );
        }
    };

    let new = crate::ports::NewProject {
        team_id: req.team_id,
        name: name.to_string(),
        slug,
        dsn_public_key: generate_dsn_key(),
        retention_events,
        retention_days,
        webhook_url,
    };

    match state.projects.create(new).await {
        Ok(project) => {
            // No project-level membership under Variant A: the creator already
            // has access (and Admin role) via their team membership / instance role.
            let view = ProjectView::from_project(project, &state.config.base_url);
            (StatusCode::CREATED, Json(view)).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// `GET /projects/{id}` — project detail, with the default (unresolved) issue
/// list embedded so the page's initial view needs no follow-up request.
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let project = match resolve_project(&state, &id).await {
        Ok(project) => project,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_member(&state, &user, project.id).await {
        return resp;
    }

    let issues = match crate::api::issues::default_project_issues(&state, &project).await {
        Ok(issues) => issues,
        Err(err) => return error_response(err),
    };

    let view = ProjectDetailView {
        project: ProjectView::from_project(project, &state.config.base_url),
        issues,
    };
    Json(view).into_response()
}

/// Request body for `PATCH /projects/{id}` (settings, retention, mute).
#[derive(Debug, Deserialize, Default)]
pub struct UpdateProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub retention_events: Option<i64>,
    /// Age-based retention in days; 0 disables, `None` leaves it.
    #[serde(default)]
    pub retention_days: Option<i64>,
    #[serde(default)]
    pub muted: Option<bool>,
    /// Per-project webhook URL. An absent key leaves it unchanged; a
    /// present value sets it, and a present empty/whitespace string clears the
    /// webhook (disables the channel) — see [`update`] for the mapping.
    #[serde(default)]
    pub webhook_url: Option<String>,
}

/// `PATCH /projects/{id}` — update settings/retention/mute (admin).
pub async fn update(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Response {
    let project_id = match resolve_project(&state, &id).await {
        Ok(project) => project.id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_admin(&state, &user, project_id).await {
        return resp;
    }

    if let Some(name) = &req.name
        && name.trim().is_empty()
    {
        return json_error(StatusCode::BAD_REQUEST, "name must not be empty");
    }
    if let Some(retention) = req.retention_events
        && retention <= 0
    {
        return json_error(StatusCode::BAD_REQUEST, "retention_events must be positive");
    }
    // Age-based retention: 0 disables, so only negatives are rejected.
    if let Some(days) = req.retention_days
        && days < 0
    {
        return json_error(
            StatusCode::BAD_REQUEST,
            "retention_days must not be negative",
        );
    }

    // Map the flat request field into ProjectUpdate's double-`Option`: an absent
    // key (`None`) leaves the webhook unchanged; a trimmed empty string maps to
    // `Some(None)` (clear intent); a present non-empty value must be a valid
    // http(s) URL and maps to `Some(Some(url))`. The adapter's CASE-based update
    // honours all three: leave / clear-to-NULL / set.
    let webhook_url = match req.webhook_url.as_deref().map(str::trim) {
        None => None,
        Some("") => Some(None),
        Some(url) if is_valid_webhook_url(url) => Some(Some(url.to_string())),
        Some(_) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "webhook_url must be a valid http(s) URL",
            );
        }
    };

    let update = crate::ports::ProjectUpdate {
        name: req.name.map(|n| n.trim().to_string()),
        retention_events: req.retention_events,
        retention_days: req.retention_days,
        muted: req.muted,
        webhook_url,
    };

    match state.projects.update(project_id, update).await {
        Ok(project) => {
            let view = ProjectView::from_project(project, &state.config.base_url);
            Json(view).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// `DELETE /projects/{id}` — delete a project (admin).
pub async fn delete(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let project_id = match resolve_project(&state, &id).await {
        Ok(project) => project.id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_admin(&state, &user, project_id).await {
        return resp;
    }

    match state.projects.delete(project_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => error_response(err),
    }
}

/// `GET /projects/{id}/issues` — list issues for a project (filter by status).
///
/// Delegated to [`crate::api::issues::list_for_project`] so issue DTO shaping
/// lives in one place.
pub async fn list_issues(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(filter): Query<crate::api::issues::IssueListQuery>,
) -> Response {
    crate::api::issues::list_for_project(state, user, id, filter).await
}

/// DSN payload returned for SDK configuration.
#[derive(Debug, Serialize)]
pub struct DsnView {
    pub dsn: String,
    pub public_key: String,
    pub project_id: Id,
}

/// `GET /projects/{id}/dsn` — DSN string for SDK configuration.
pub async fn dsn(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let project = match resolve_project(&state, &id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_member(&state, &user, project.id).await {
        return resp;
    }

    let dsn = build_dsn(&state.config.base_url, &project.dsn_public_key, project.id);
    Json(DsnView {
        dsn,
        public_key: project.dsn_public_key,
        project_id: project.id,
    })
    .into_response()
}

/// A single language-specific SDK setup snippet.
#[derive(Debug, Serialize)]
pub struct SdkSnippet {
    pub language: &'static str,
    pub label: &'static str,
    pub code: String,
}

/// SDK setup payload: the DSN plus per-language init snippets.
#[derive(Debug, Serialize)]
pub struct SdkSetupView {
    pub dsn: String,
    pub snippets: Vec<SdkSnippet>,
}

/// `GET /projects/{id}/sdk-setup` — language-aware SDK setup snippets.
///
/// Returns the DSN plus minimal init code for Go, Rust, Svelte/JS, and a
/// generic example.
pub async fn sdk_setup(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let project = match resolve_project(&state, &id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_member(&state, &user, project.id).await {
        return resp;
    }

    let dsn = build_dsn(&state.config.base_url, &project.dsn_public_key, project.id);
    let (envelope_url, store_url) = ingest_urls(&state.config.base_url, project.id);
    let snippets = sdk_snippets(&dsn, &project.dsn_public_key, &envelope_url, &store_url);
    Json(SdkSetupView { dsn, snippets }).into_response()
}

/// Build the per-language SDK init snippets for a DSN.
///
/// The language snippets (Go/Rust/JS) embed only the DSN — every modern Sentry
/// SDK targets the `/envelope/` endpoint automatically. The `generic` snippet
/// documents the raw HTTP contract for both the modern `/envelope/` and the
/// legacy `/store/` endpoints, for callers without an SDK.
fn sdk_snippets(
    dsn: &str,
    public_key: &str,
    envelope_url: &str,
    store_url: &str,
) -> Vec<SdkSnippet> {
    vec![
        SdkSnippet {
            language: "go",
            label: "Go",
            code: format!(
                "import \"github.com/getsentry/sentry-go\"\n\n\
                 err := sentry.Init(sentry.ClientOptions{{\n    \
                     Dsn: \"{dsn}\",\n}})\n\
                 if err != nil {{\n    \
                     log.Fatalf(\"sentry.Init: %s\", err)\n}}\n\
                 defer sentry.Flush(2 * time.Second)"
            ),
        },
        SdkSnippet {
            language: "rust",
            label: "Rust",
            code: format!(
                "let _guard = sentry::init((\n    \
                     \"{dsn}\",\n    \
                     sentry::ClientOptions {{\n        \
                         release: sentry::release_name!(),\n        \
                         ..Default::default()\n    \
                     }},\n));"
            ),
        },
        SdkSnippet {
            language: "javascript",
            label: "Svelte / JavaScript",
            code: format!(
                "import * as Sentry from \"@sentry/svelte\";\n\n\
                 Sentry.init({{\n  \
                     dsn: \"{dsn}\",\n  \
                     tracesSampleRate: 0,\n}});"
            ),
        },
        SdkSnippet {
            language: "generic",
            label: "Generic / raw HTTP",
            code: format!(
                "# Point any Sentry-compatible SDK at this DSN:\n\
                 SENTRY_DSN={dsn}\n\n\
                 # --- Or send events over raw HTTP ---\n\
                 # Auth via the `X-Sentry-Auth` header (or `?sentry_key=` query).\n\n\
                 # Modern: newline-delimited envelope (recommended).\n\
                 printf '{{\"event_id\":\"%s\"}}\\n{{\"type\":\"event\"}}\\n{{\"message\":\"hello\"}}\\n' \\\n  \
                     \"$(uuidgen | tr -d - | tr 'A-Z' 'a-z')\" \\\n\
                 | curl -X POST '{envelope_url}' \\\n    \
                     -H 'X-Sentry-Auth: Sentry sentry_version=7, sentry_key={public_key}' \\\n    \
                     -H 'Content-Type: application/x-sentry-envelope' \\\n    \
                     --data-binary @-\n\n\
                 # Legacy: a single bare JSON event (pre-envelope SDKs).\n\
                 curl -X POST '{store_url}' \\\n    \
                     -H 'X-Sentry-Auth: Sentry sentry_version=7, sentry_key={public_key}' \\\n    \
                     -H 'Content-Type: application/json' \\\n    \
                     -d '{{\"message\":\"hello\"}}'"
            ),
        },
    ]
}

/// Request body for `POST /projects/{id}/mute` (project-level mute).
#[derive(Debug, Deserialize)]
pub struct MuteRequest {
    /// Target mute state. When omitted, mute is toggled.
    #[serde(default)]
    pub muted: Option<bool>,
}

/// `POST /projects/{id}/mute` — toggle project-level mute (admin).
///
/// Project mute suppresses notifications only; ingestion and counting continue.
pub async fn mute(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    body: Option<Json<MuteRequest>>,
) -> Response {
    let project = match resolve_project(&state, &id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_admin(&state, &user, project.id).await {
        return resp;
    }

    let target = body.and_then(|Json(b)| b.muted).unwrap_or(!project.muted);

    let update = crate::ports::ProjectUpdate {
        muted: Some(target),
        ..Default::default()
    };

    match state.projects.update(project.id, update).await {
        Ok(project) => {
            let view = ProjectView::from_project(project, &state.config.base_url);
            Json(view).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// `POST /projects/{id}/regenerate-dsn` — rotate the DSN public key (admin).
pub async fn regenerate_dsn(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let project_id = match resolve_project(&state, &id).await {
        Ok(project) => project.id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_admin(&state, &user, project_id).await {
        return resp;
    }

    let new_key = generate_dsn_key();
    match state.projects.regenerate_dsn(project_id, new_key).await {
        Ok(project) => {
            let dsn = build_dsn(&state.config.base_url, &project.dsn_public_key, project.id);
            Json(DsnView {
                dsn,
                public_key: project.dsn_public_key,
                project_id: project.id,
            })
            .into_response()
        }
        Err(err) => error_response(err),
    }
}

/// Request body for `POST /projects/{id}/favorite`.
#[derive(Debug, Deserialize, Default)]
pub struct FavoriteRequest {
    /// Desired favorite state. When absent the current state is toggled.
    #[serde(default)]
    pub favorited: Option<bool>,
}

/// Response body for `POST /projects/{id}/favorite`.
#[derive(Debug, Serialize)]
pub struct FavoriteResponse {
    pub favorited: bool,
}

/// `POST /projects/{id}/favorite` — add/remove a project from the caller's favorites.
///
/// Accepts an optional `{ favorited: bool }` body. When `favorited` is omitted
/// the current state is toggled. The caller must be a project member.
pub async fn favorite(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    body: Option<Json<FavoriteRequest>>,
) -> Response {
    let project_id = match resolve_project(&state, &id).await {
        Ok(project) => project.id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_member(&state, &user, project_id).await {
        return resp;
    }

    let target = match body.and_then(|Json(b)| b.favorited) {
        Some(v) => v,
        None => {
            let favorites = match state.favorites.list_for_user(user.id).await {
                Ok(f) => f,
                Err(err) => return error_response(err),
            };
            !favorites.contains(&project_id)
        }
    };

    let result = if target {
        state.favorites.add(user.id, project_id).await
    } else {
        state.favorites.remove(user.id, project_id).await
    };

    match result {
        Ok(()) => Json(FavoriteResponse { favorited: target }).into_response(),
        Err(err) => error_response(err),
    }
}

// ---------------------------------------------------------------------------
// Small pure helpers (slug + key generation)
// ---------------------------------------------------------------------------

/// Generate a fresh DSN public key (Sentry uses a 32-char hex string).
fn generate_dsn_key() -> String {
    Id::new_v4().simple().to_string()
}

/// Derive a URL-friendly slug from a free-text name.
fn slugify(input: &str) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        // Fall back to a random suffix so slugs are never empty.
        return format!("project-{}", &Id::new_v4().simple().to_string()[..8]);
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::InstanceRole;
    use uuid::Uuid;

    #[test]
    fn effective_role_truth_table() {
        use InstanceRole::*;
        // Instance Owner|Manager => Admin on every project, regardless of team role.
        assert_eq!(resolve_role(Owner, None), Some(Role::Admin));
        assert_eq!(
            resolve_role(Owner, Some(TeamRole::Contributor)),
            Some(Role::Admin)
        );
        assert_eq!(resolve_role(Manager, None), Some(Role::Admin));
        assert_eq!(
            resolve_role(Manager, Some(TeamRole::Contributor)),
            Some(Role::Admin)
        );
        // Plain Member: access derives purely from the team role.
        assert_eq!(
            resolve_role(Member, Some(TeamRole::Admin)),
            Some(Role::Admin)
        );
        assert_eq!(
            resolve_role(Member, Some(TeamRole::Contributor)),
            Some(Role::Member)
        );
        // No team membership and no instance privilege => no access (closed membership).
        assert_eq!(resolve_role(Member, None), None);
    }

    #[test]
    fn instance_gates_follow_role_hierarchy() {
        let owner = user(InstanceRole::Owner);
        let manager = user(InstanceRole::Manager);
        let member = user(InstanceRole::Member);
        assert!(require_manager(&owner).is_ok());
        assert!(require_manager(&manager).is_ok());
        assert!(require_manager(&member).is_err());
        assert!(require_owner(&owner).is_ok());
        assert!(require_owner(&manager).is_err());
        assert!(require_owner(&member).is_err());
    }

    fn user(instance_role: InstanceRole) -> User {
        let now = chrono::Utc::now();
        User {
            id: Uuid::new_v4(),
            email: "u@example.com".into(),
            display_name: "U".into(),
            password_hash: String::new(),
            instance_role,
            notifications_enabled: true,
            auth_provider: crate::domain::AuthProvider::Local,
            status: crate::domain::UserStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn project_detail_view_flattens_project_alongside_issues() {
        // The frontend `ProjectDetail` type expects the project fields at the top
        // level (flattened) plus a sibling `issues` array — assert that contract.
        let now = chrono::Utc::now();
        let project = ProjectView {
            id: "n".into(),
            team_id: Uuid::nil(),
            name: "n".into(),
            slug: "n".into(),
            dsn_public_key: "key".into(),
            dsn: "http://key@host/0".into(),
            retention_events: 1,
            retention_days: 0,
            muted: false,
            webhook_url: None,
            created_at: now,
            updated_at: now,
        };
        let view = ProjectDetailView {
            project,
            issues: vec![],
        };

        let json = serde_json::to_value(&view).expect("serialize");
        assert!(json.get("id").is_some(), "flattened project id present");
        assert!(json.get("dsn").is_some(), "flattened project dsn present");
        assert!(
            json.get("issues").is_some_and(|v| v.is_array()),
            "issues array present at top level"
        );
    }

    #[test]
    fn build_dsn_injects_public_key_and_strips_trailing_slash() {
        let pid = Uuid::nil();
        let dsn = build_dsn("https://errors.example.com/", "abc123", pid);
        assert_eq!(dsn, format!("https://abc123@errors.example.com/{pid}"));
    }

    #[test]
    fn build_dsn_defaults_scheme_when_missing() {
        let pid = Uuid::nil();
        let dsn = build_dsn("localhost:8080", "key", pid);
        assert_eq!(dsn, format!("http://key@localhost:8080/{pid}"));
    }

    #[test]
    fn build_dsn_preserves_port() {
        let pid = Uuid::nil();
        let dsn = build_dsn("http://localhost:8080", "k", pid);
        assert_eq!(dsn, format!("http://k@localhost:8080/{pid}"));
    }

    #[test]
    fn slugify_normalizes_text() {
        assert_eq!(slugify("My Cool Project!"), "my-cool-project");
        assert_eq!(slugify("  spaced   out  "), "spaced-out");
        assert_eq!(slugify("Already-Slug"), "already-slug");
    }

    #[test]
    fn slugify_empty_falls_back_to_random() {
        let s = slugify("!!!");
        assert!(s.starts_with("project-"));
    }

    #[test]
    fn sdk_snippets_cover_all_languages_and_embed_dsn() {
        let dsn = "http://key@host/1";
        let envelope_url = "http://host/api/1/envelope/";
        let store_url = "http://host/api/1/store/";
        let snippets = sdk_snippets(dsn, "key", envelope_url, store_url);
        let langs: Vec<&str> = snippets.iter().map(|s| s.language).collect();
        assert_eq!(langs, vec!["go", "rust", "javascript", "generic"]);
        for snippet in &snippets {
            assert!(
                snippet.code.contains(dsn),
                "{} missing dsn",
                snippet.language
            );
        }

        // The generic snippet documents both ingestion endpoints over raw HTTP.
        let generic = snippets
            .iter()
            .find(|s| s.language == "generic")
            .expect("generic snippet");
        assert!(generic.code.contains(envelope_url), "missing envelope url");
        assert!(generic.code.contains(store_url), "missing store url");
    }

    #[test]
    fn ingest_urls_builds_both_endpoints_and_strips_trailing_slash() {
        let pid = Uuid::nil();
        let (envelope, store) = ingest_urls("https://errors.example.com/", pid);
        assert_eq!(
            envelope,
            format!("https://errors.example.com/api/{pid}/envelope/")
        );
        assert_eq!(
            store,
            format!("https://errors.example.com/api/{pid}/store/")
        );
    }

    #[test]
    fn generate_dsn_key_is_32_hex_chars() {
        let key = generate_dsn_key();
        assert_eq!(key.len(), 32);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn error_response_maps_statuses() {
        use axum::http::StatusCode;
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
            let resp = error_response(err);
            assert_eq!(resp.status(), expected);
        }
    }

    #[test]
    fn valid_webhook_url_accepts_http_and_https() {
        assert!(is_valid_webhook_url("https://hooks.example.com/abc"));
        assert!(is_valid_webhook_url("http://10.0.0.1:9000/ingest"));
    }

    #[test]
    fn valid_webhook_url_rejects_missing_scheme_and_other_schemes() {
        // No scheme, wrong scheme, and outright garbage are all rejected so the
        // misconfiguration surfaces at save time rather than as a silent
        // per-event delivery failure.
        assert!(!is_valid_webhook_url("hooks.example.com/abc"));
        assert!(!is_valid_webhook_url("ftp://example.com/x"));
        assert!(!is_valid_webhook_url("not a url"));
    }
}
