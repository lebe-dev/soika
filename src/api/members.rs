//! Project membership API handlers.
//!
//! Listing requires any project role; removal requires project admin.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::api::projects::{
    CurrentUser, error_response, json_error, parse_id, require_admin, require_member,
    resolve_project,
};
use crate::domain::{Id, Role};
use crate::state::AppState;

/// A project member as returned to the client (user fields + role).
#[derive(Debug, Serialize)]
pub struct MemberView {
    pub user_id: Id,
    pub email: String,
    pub display_name: String,
    pub role: Role,
}

/// `GET /projects/{id}/members` — list project members & roles.
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_id): Path<String>,
) -> Response {
    let project_id = match resolve_project(&state, &project_id).await {
        Ok(project) => project.id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_member(&state, &user, project_id).await {
        return resp;
    }

    match state.memberships.members(project_id).await {
        Ok(members) => {
            let views: Vec<MemberView> = members
                .into_iter()
                .map(|(u, role)| MemberView {
                    user_id: u.id,
                    email: u.email,
                    display_name: u.display_name,
                    role,
                })
                .collect();
            Json(views).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// `DELETE /projects/{id}/members/{user_id}` — remove a member (admin).
///
/// Guards against removing the project's last admin so a project cannot be
/// orphaned without an administrator.
pub async fn remove(
    State(state): State<AppState>,
    CurrentUser(caller): CurrentUser,
    Path((project_id, user_id)): Path<(String, String)>,
) -> Response {
    let project_id = match resolve_project(&state, &project_id).await {
        Ok(project) => project.id,
        Err(resp) => return resp,
    };
    let target_user = match parse_id(&user_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_admin(&state, &caller, project_id).await {
        return resp;
    }

    let memberships = match state.memberships.list_for_project(project_id).await {
        Ok(m) => m,
        Err(err) => return error_response(err),
    };

    let Some(target) = memberships.iter().find(|m| m.user_id == target_user) else {
        return json_error(StatusCode::NOT_FOUND, "member not found in project");
    };

    // Prevent orphaning the project: refuse to remove the last admin.
    if target.role == Role::Admin {
        let admin_count = memberships.iter().filter(|m| m.role == Role::Admin).count();
        if admin_count <= 1 {
            return json_error(
                StatusCode::CONFLICT,
                "cannot remove the last admin of the project",
            );
        }
    }

    match state.memberships.remove(project_id, target_user).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => error_response(err),
    }
}
