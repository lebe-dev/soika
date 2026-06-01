//! Issue API handlers (MVP §16 Issues, §8.1).
//!
//! Session-cookie auth; access scoped to projects the caller can view (§10.2).
//! Members and admins may both resolve/mute issues (§10.2).

// Guards return `Result<T, Response>`; axum's `Response` is large — acceptable
// for these short-circuit helpers.
#![allow(clippy::result_large_err)]

use std::str::FromStr;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::projects::{error_response, parse_id, require_member, CurrentUser};
use crate::domain::{Event, Id, Issue, IssueStatus};
use crate::ports::IssueFilter;
use crate::state::AppState;

/// Query parameters for the issues list (§8 filter by status).
#[derive(Debug, Deserialize, Default)]
pub struct IssueListQuery {
    /// Filter by status: `unresolved` | `resolved` | `muted`.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Issue as returned to the client (currently a straight projection).
#[derive(Debug, Serialize)]
pub struct IssueView {
    pub id: Id,
    pub project_id: Id,
    pub fingerprint: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: Option<String>,
    pub status: IssueStatus,
    pub first_seen: crate::domain::Timestamp,
    pub last_seen: crate::domain::Timestamp,
    pub event_count: i64,
}

impl From<Issue> for IssueView {
    fn from(issue: Issue) -> Self {
        IssueView {
            id: issue.id,
            project_id: issue.project_id,
            fingerprint: issue.fingerprint,
            title: issue.title,
            culprit: issue.culprit,
            level: issue.level,
            status: issue.status,
            first_seen: issue.first_seen,
            last_seen: issue.last_seen,
            event_count: issue.event_count,
        }
    }
}

/// Event as returned to the client (full payload preserved — §6).
#[derive(Debug, Serialize)]
pub struct EventView {
    pub id: Id,
    pub event_id: String,
    pub issue_id: Id,
    pub project_id: Id,
    pub payload: serde_json::Value,
    pub received_at: crate::domain::Timestamp,
}

impl From<Event> for EventView {
    fn from(event: Event) -> Self {
        EventView {
            id: event.id,
            event_id: event.event_id,
            issue_id: event.issue_id,
            project_id: event.project_id,
            payload: event.payload,
            received_at: event.received_at,
        }
    }
}

/// Translate an [`IssueListQuery`] into a domain [`IssueFilter`], validating the
/// status string.
fn to_filter(query: IssueListQuery) -> std::result::Result<IssueFilter, Response> {
    let status = match query.status.as_deref() {
        Some(s) if !s.trim().is_empty() => Some(IssueStatus::from_str(s.trim()).map_err(|_| {
            crate::api::projects::json_error(StatusCode::BAD_REQUEST, "invalid status filter")
        })?),
        _ => None,
    };

    if let Some(limit) = query.limit
        && limit < 0
    {
        return Err(crate::api::projects::json_error(
            StatusCode::BAD_REQUEST,
            "limit must be non-negative",
        ));
    }
    if let Some(offset) = query.offset
        && offset < 0
    {
        return Err(crate::api::projects::json_error(
            StatusCode::BAD_REQUEST,
            "offset must be non-negative",
        ));
    }

    Ok(IssueFilter {
        status,
        query: query.query.filter(|q| !q.trim().is_empty()),
        limit: query.limit,
        offset: query.offset,
    })
}

/// Shared implementation for `GET /projects/{id}/issues` (§8 issues list).
///
/// Invoked from [`crate::api::projects::list_issues`] so the issue DTO lives
/// here in one place.
pub async fn list_for_project(
    state: AppState,
    CurrentUser(user): CurrentUser,
    raw_project_id: String,
    query: IssueListQuery,
) -> Response {
    let project_id = match parse_id(&raw_project_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_member(&state, &user, project_id).await {
        return resp;
    }

    let filter = match to_filter(query) {
        Ok(f) => f,
        Err(resp) => return resp,
    };

    match state.issues.list(project_id, filter).await {
        Ok(issues) => {
            let views: Vec<IssueView> = issues.into_iter().map(IssueView::from).collect();
            Json(views).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// Load an issue and verify the caller may view its project; returns the issue.
async fn authorize_issue(
    state: &AppState,
    user: &crate::domain::User,
    raw_id: &str,
) -> std::result::Result<Issue, Response> {
    let issue_id = parse_id(raw_id)?;
    let issue = state
        .issues
        .find_by_id(issue_id)
        .await
        .map_err(error_response)?
        .ok_or_else(|| {
            crate::api::projects::json_error(StatusCode::NOT_FOUND, "issue not found")
        })?;
    require_member(state, user, issue.project_id).await?;
    Ok(issue)
}

/// `GET /issues/{id}` — issue detail (issue + latest event for the viewer).
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let issue = match authorize_issue(&state, &user, &id).await {
        Ok(issue) => issue,
        Err(resp) => return resp,
    };

    let latest = match state.events.latest_for_issue(issue.id).await {
        Ok(ev) => ev.map(EventView::from),
        Err(err) => return error_response(err),
    };

    #[derive(Serialize)]
    struct IssueDetail {
        #[serde(flatten)]
        issue: IssueView,
        latest_event: Option<EventView>,
    }

    Json(IssueDetail {
        issue: IssueView::from(issue),
        latest_event: latest,
    })
    .into_response()
}

/// Query parameters for `GET /issues/{id}/events`.
#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    #[serde(default = "default_events_limit")]
    pub limit: i64,
}

fn default_events_limit() -> i64 {
    50
}

/// `GET /issues/{id}/events` — events for an issue (newest first).
pub async fn list_events(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> Response {
    let issue = match authorize_issue(&state, &user, &id).await {
        Ok(issue) => issue,
        Err(resp) => return resp,
    };

    let limit = q.limit.clamp(1, 200);
    match state.events.recent_events(issue.id, limit).await {
        Ok(events) => {
            let views: Vec<EventView> = events.into_iter().map(EventView::from).collect();
            Json(views).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// `POST /issues/{id}/resolve` — mark resolved (§8.1).
pub async fn resolve(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    set_status(state, user, id, IssueStatus::Resolved).await
}

/// `POST /issues/{id}/mute` — mute the issue (§8.1).
///
/// Mute keeps accepting and counting events; it only suppresses notifications.
/// It never affects ingestion.
pub async fn mute(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    set_status(state, user, id, IssueStatus::Muted).await
}

/// `POST /issues/{id}/unresolve` — re-open a resolved/muted issue.
pub async fn unresolve(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    set_status(state, user, id, IssueStatus::Unresolved).await
}

/// Shared status-transition helper for resolve/mute/unresolve.
async fn set_status(
    state: AppState,
    user: crate::domain::User,
    raw_id: String,
    status: IssueStatus,
) -> Response {
    let issue = match authorize_issue(&state, &user, &raw_id).await {
        Ok(issue) => issue,
        Err(resp) => return resp,
    };

    match state.issues.set_status(issue.id, status).await {
        Ok(issue) => Json(IssueView::from(issue)).into_response(),
        Err(err) => error_response(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_filter_parses_status_and_passes_paging() {
        let q = IssueListQuery {
            status: Some("resolved".into()),
            query: Some("boom".into()),
            limit: Some(10),
            offset: Some(5),
        };
        let f = to_filter(q).expect("valid filter");
        assert_eq!(f.status, Some(IssueStatus::Resolved));
        assert_eq!(f.query.as_deref(), Some("boom"));
        assert_eq!(f.limit, Some(10));
        assert_eq!(f.offset, Some(5));
    }

    #[test]
    fn to_filter_empty_status_is_none() {
        let q = IssueListQuery {
            status: Some("  ".into()),
            ..Default::default()
        };
        let f = to_filter(q).expect("valid");
        assert_eq!(f.status, None);
    }

    #[test]
    fn to_filter_rejects_invalid_status() {
        let q = IssueListQuery {
            status: Some("nope".into()),
            ..Default::default()
        };
        let resp = to_filter(q).expect_err("should reject");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn to_filter_rejects_negative_paging() {
        let neg_limit = IssueListQuery {
            limit: Some(-1),
            ..Default::default()
        };
        assert_eq!(
            to_filter(neg_limit).expect_err("reject").status(),
            StatusCode::BAD_REQUEST
        );

        let neg_offset = IssueListQuery {
            offset: Some(-1),
            ..Default::default()
        };
        assert_eq!(
            to_filter(neg_offset).expect_err("reject").status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn to_filter_blank_query_becomes_none() {
        let q = IssueListQuery {
            query: Some("   ".into()),
            ..Default::default()
        };
        assert_eq!(to_filter(q).expect("valid").query, None);
    }

    #[test]
    fn default_events_limit_is_fifty() {
        assert_eq!(default_events_limit(), 50);
    }
}
