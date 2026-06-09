//! Issue API handlers (Issues).
//!
//! Session-cookie auth; access scoped to projects the caller can view.
//! Members and admins may both resolve/mute issues.

// Guards return `Result<T, Response>`; axum's `Response` is large — acceptable
// for these short-circuit helpers.
#![allow(clippy::result_large_err)]

use std::str::FromStr;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::api::projects::{CurrentUser, error_response, require_member, resolve_project};
use crate::domain::{Event, Id, Issue, IssueStatus, Project};
use crate::ports::{IssueFilter, IssueSort};
use crate::state::AppState;

/// Query parameters for the issues list (filter by status).
#[derive(Debug, Deserialize, Default)]
pub struct IssueListQuery {
    /// Filter by status: `unresolved` | `resolved` | `muted`.
    #[serde(default)]
    pub status: Option<String>,
    /// Exact-match severity level (`error`, `warning`, ...); free-form per Sentry.
    #[serde(default)]
    pub level: Option<String>,
    /// Exact-match environment (`production`, `staging`, ...).
    #[serde(default)]
    pub environment: Option<String>,
    /// Exact-match release (version/build identifier).
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    /// Sort order: `recency`/`last_seen` (default) or `frequency`/`event_count`.
    #[serde(default)]
    pub sort: Option<String>,
}

/// Issue as returned to the client.
///
/// Both ids are short public ids, not UUIDs: `id` is the issue's own short id and
/// `project_id` is the parent project's short id — the frontend uses the latter
/// to build the back-link to the project page. See [`IssueView::from_issue`].
#[derive(Debug, Serialize)]
pub struct IssueView {
    pub id: String,
    pub project_id: String,
    pub fingerprint: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub status: IssueStatus,
    pub first_seen: crate::domain::Timestamp,
    pub last_seen: crate::domain::Timestamp,
    pub event_count: i64,
}

impl IssueView {
    /// Build a view from an issue plus its parent project's short id.
    ///
    /// `project_short_id` cannot be derived from the issue alone (it carries the
    /// project UUID), so callers pass it in from the resolved/loaded project.
    pub fn from_issue(issue: Issue, project_short_id: String) -> Self {
        IssueView {
            id: issue.short_id,
            project_id: project_short_id,
            fingerprint: issue.fingerprint,
            title: issue.title,
            culprit: issue.culprit,
            level: issue.level,
            environment: issue.environment,
            release: issue.release,
            status: issue.status,
            first_seen: issue.first_seen,
            last_seen: issue.last_seen,
            event_count: issue.event_count,
        }
    }
}

/// Event as returned to the client (full payload preserved).
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

    let sort = match query.sort.as_deref().map(str::trim) {
        None | Some("") => IssueSort::default(),
        Some("recency") | Some("last_seen") => IssueSort::LastSeen,
        Some("frequency") | Some("event_count") => IssueSort::EventCount,
        Some(_) => {
            return Err(crate::api::projects::json_error(
                StatusCode::BAD_REQUEST,
                "invalid sort",
            ));
        }
    };

    // level/environment/release are free-form per Sentry — do NOT validate
    // against an enum (that would reject valid levels). Only blank-guard them so
    // empty query params become None (mirrors the `query` blank-guard).
    let blank_to_none = |s: Option<String>| s.filter(|v| !v.trim().is_empty());

    Ok(IssueFilter {
        status,
        level: blank_to_none(query.level),
        environment: blank_to_none(query.environment),
        release: blank_to_none(query.release),
        query: query.query.filter(|q| !q.trim().is_empty()),
        limit: query.limit,
        offset: query.offset,
        sort,
    })
}

/// Shared implementation for `GET /projects/{id}/issues` (issues list).
///
/// Invoked from [`crate::api::projects::list_issues`] so the issue DTO lives
/// here in one place.
pub async fn list_for_project(
    state: AppState,
    CurrentUser(user): CurrentUser,
    raw_project_id: String,
    query: IssueListQuery,
) -> Response {
    let project = match resolve_project(&state, &raw_project_id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_member(&state, &user, project.id).await {
        return resp;
    }

    let filter = match to_filter(query) {
        Ok(f) => f,
        Err(resp) => return resp,
    };

    match state.issues.list(project.id, filter).await {
        Ok(issues) => {
            let views: Vec<IssueView> = issues
                .into_iter()
                .map(|i| IssueView::from_issue(i, project.short_id.clone()))
                .collect();
            Json(views).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// Build the default issue list for a project — unresolved issues, most-recent
/// first — matching the project page's initial (unfiltered) view.
///
/// Used by [`crate::api::projects::get`] to seed the project detail response so
/// the page renders its default issue list without a follow-up request. The
/// caller is responsible for access checks (the project detail handler already
/// runs `require_member`).
pub async fn default_project_issues(
    state: &AppState,
    project: &Project,
) -> Result<Vec<IssueView>, crate::error::Error> {
    let filter = IssueFilter {
        status: Some(IssueStatus::Unresolved),
        ..IssueFilter::default()
    };
    let issues = state.issues.list(project.id, filter).await?;
    Ok(issues
        .into_iter()
        .map(|i| IssueView::from_issue(i, project.short_id.clone()))
        .collect())
}

/// Resolve an issue by its short public id and verify the caller may view its
/// project; returns the issue together with its parent project.
///
/// The project is returned so callers can build [`IssueView`]s (which need the
/// project's short id) without a second lookup.
async fn authorize_issue(
    state: &AppState,
    user: &crate::domain::User,
    raw_id: &str,
) -> std::result::Result<(Issue, Project), Response> {
    let issue = state
        .issues
        .find_by_short_id(raw_id)
        .await
        .map_err(error_response)?
        .ok_or_else(|| {
            crate::api::projects::json_error(StatusCode::NOT_FOUND, "issue not found")
        })?;
    let project = state
        .projects
        .find_by_id(issue.project_id)
        .await
        .map_err(error_response)?
        .ok_or_else(|| {
            crate::api::projects::json_error(StatusCode::NOT_FOUND, "project not found")
        })?;
    require_member(state, user, project.id).await?;
    Ok((issue, project))
}

/// `GET /issues/{id}` — issue detail (issue + latest event for the viewer).
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let (issue, project) = match authorize_issue(&state, &user, &id).await {
        Ok(pair) => pair,
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
        issue: IssueView::from_issue(issue, project.short_id),
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
    let (issue, _project) = match authorize_issue(&state, &user, &id).await {
        Ok(pair) => pair,
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

/// `POST /issues/{id}/resolve` — mark resolved.
pub async fn resolve(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    set_status(state, user, id, IssueStatus::Resolved).await
}

/// `POST /issues/{id}/mute` — mute the issue.
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

/// Request body for `PATCH /issues/{id}/fingerprint`.
#[derive(Debug, Deserialize)]
pub struct UpdateFingerprintRequest {
    pub fingerprint: String,
}

/// `PATCH /issues/{id}/fingerprint` — override an issue fingerprint (merge /
/// split). If another issue in the project already holds the new fingerprint,
/// the two are merged and the surviving issue is returned (its id may differ
/// from `{id}` only in that the *other* row is the one removed; the row
/// addressed by `{id}` always survives). Authorized via project membership.
///
/// NOTE: the JSON body extractor consumes the request body, so it MUST be the
/// LAST extractor (mirrors `projects::update`).
pub async fn update_fingerprint(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateFingerprintRequest>,
) -> Response {
    let trimmed = req.fingerprint.trim();
    if trimmed.is_empty() {
        return crate::api::projects::json_error(
            StatusCode::BAD_REQUEST,
            "fingerprint must not be empty",
        );
    }

    let (issue, project) = match authorize_issue(&state, &user, &id).await {
        Ok(pair) => pair,
        Err(resp) => return resp,
    };

    match state
        .issues
        .override_fingerprint(issue.id, trimmed.to_string())
        .await
    {
        // The surviving issue stays in the same project, so its short id is the
        // one we already loaded.
        Ok(issue) => Json(IssueView::from_issue(issue, project.short_id)).into_response(),
        Err(err) => error_response(err),
    }
}

/// Shared status-transition helper for resolve/mute/unresolve.
async fn set_status(
    state: AppState,
    user: crate::domain::User,
    raw_id: String,
    status: IssueStatus,
) -> Response {
    let (issue, project) = match authorize_issue(&state, &user, &raw_id).await {
        Ok(pair) => pair,
        Err(resp) => return resp,
    };

    match state.issues.set_status(issue.id, status).await {
        Ok(issue) => Json(IssueView::from_issue(issue, project.short_id)).into_response(),
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
            sort: None,
            ..Default::default()
        };
        let f = to_filter(q).expect("valid filter");
        assert_eq!(f.status, Some(IssueStatus::Resolved));
        assert_eq!(f.query.as_deref(), Some("boom"));
        assert_eq!(f.limit, Some(10));
        assert_eq!(f.offset, Some(5));
        assert_eq!(f.sort, IssueSort::LastSeen);
    }

    #[test]
    fn to_filter_maps_level_environment_release() {
        let q = IssueListQuery {
            level: Some("error".into()),
            environment: Some("production".into()),
            release: Some("1.2.3".into()),
            ..Default::default()
        };
        let f = to_filter(q).expect("valid filter");
        assert_eq!(f.level.as_deref(), Some("error"));
        assert_eq!(f.environment.as_deref(), Some("production"));
        assert_eq!(f.release.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn to_filter_blank_level_environment_release_become_none() {
        let q = IssueListQuery {
            level: Some("  ".into()),
            environment: Some("".into()),
            release: Some("\t".into()),
            ..Default::default()
        };
        let f = to_filter(q).expect("valid filter");
        assert_eq!(f.level, None);
        assert_eq!(f.environment, None);
        assert_eq!(f.release, None);
    }

    #[test]
    fn to_filter_default_sort_is_last_seen() {
        let q = IssueListQuery {
            sort: None,
            ..Default::default()
        };
        let f = to_filter(q).expect("valid");
        assert_eq!(f.sort, IssueSort::LastSeen);
        assert_eq!(f.sort, IssueFilter::default().sort);
    }

    #[test]
    fn to_filter_maps_recency_and_last_seen_to_last_seen() {
        for value in ["recency", "last_seen"] {
            let q = IssueListQuery {
                sort: Some(value.into()),
                ..Default::default()
            };
            assert_eq!(
                to_filter(q).expect("valid").sort,
                IssueSort::LastSeen,
                "{value} should map to LastSeen"
            );
        }
    }

    #[test]
    fn to_filter_maps_frequency_and_event_count_to_event_count() {
        for value in ["frequency", "event_count"] {
            let q = IssueListQuery {
                sort: Some(value.into()),
                ..Default::default()
            };
            assert_eq!(
                to_filter(q).expect("valid").sort,
                IssueSort::EventCount,
                "{value} should map to EventCount"
            );
        }
    }

    #[test]
    fn to_filter_rejects_invalid_sort() {
        let q = IssueListQuery {
            sort: Some("sideways".into()),
            ..Default::default()
        };
        let resp = to_filter(q).expect_err("should reject");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
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
