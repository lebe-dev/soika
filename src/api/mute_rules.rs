//! Tag-mute rule API handlers.
//!
//! Project-level "mute by tags" rules: list / create / delete. Session-cookie
//! auth; member+ may manage them (same bar as resolve/mute, via
//! [`require_member`]). Each rule is a set of `key=value` tag pairs (AND); when
//! any rule matches an incoming event's tags, the project's notification for
//! that event is suppressed (the event is still ingested and counted).

// Guards return `Result<T, Response>`; axum's `Response` is large — acceptable
// for these short-circuit helpers.
#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::api::projects::{
    CurrentUser, error_response, json_error, parse_id, require_member, resolve_project,
};
use crate::domain::{TagMatch, TagMuteRule, Timestamp};
use crate::ports::NewTagMuteRule;
use crate::state::AppState;

/// A tag-mute rule as returned to the client. The rule id is its internal UUID
/// (rules have no short id); the project is addressed by short id in the path.
#[derive(Debug, Serialize)]
pub struct TagMuteRuleView {
    pub id: String,
    pub name: Option<String>,
    pub tags: Vec<TagMatch>,
    pub created_at: Timestamp,
}

impl From<TagMuteRule> for TagMuteRuleView {
    fn from(rule: TagMuteRule) -> Self {
        TagMuteRuleView {
            id: rule.id.to_string(),
            name: rule.name,
            tags: rule.tags,
            created_at: rule.created_at,
        }
    }
}

/// Request body for creating a rule: an optional label and one-or-more tag pairs.
#[derive(Debug, Deserialize)]
pub struct CreateRuleRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub tags: Vec<TagMatch>,
}

/// `GET /projects/{id}/mute-rules` — list a project's tag-mute rules.
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_short_id): Path<String>,
) -> Result<Response, Response> {
    let project = resolve_project(&state, &project_short_id).await?;
    require_member(&state, &user, project.id).await?;

    let rules = state
        .mute_rules
        .list_for_project(project.id)
        .await
        .map_err(error_response)?;

    let views: Vec<TagMuteRuleView> = rules.into_iter().map(TagMuteRuleView::from).collect();
    Ok(Json(views).into_response())
}

/// `POST /projects/{id}/mute-rules` — create a tag-mute rule (member+).
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(project_short_id): Path<String>,
    Json(body): Json<CreateRuleRequest>,
) -> Result<Response, Response> {
    let project = resolve_project(&state, &project_short_id).await?;
    require_member(&state, &user, project.id).await?;

    let tags = validate_tags(body.tags)?;
    let name = body
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());

    let rule = state
        .mute_rules
        .create(NewTagMuteRule {
            project_id: project.id,
            name,
            created_by: Some(user.id),
            tags,
        })
        .await
        .map_err(error_response)?;

    Ok((StatusCode::CREATED, Json(TagMuteRuleView::from(rule))).into_response())
}

/// `DELETE /projects/{id}/mute-rules/{rule_id}` — remove a rule (member+).
pub async fn delete(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((project_short_id, rule_id)): Path<(String, String)>,
) -> Result<Response, Response> {
    let project = resolve_project(&state, &project_short_id).await?;
    require_member(&state, &user, project.id).await?;
    let rule_id = parse_id(&rule_id)?;

    let removed = state
        .mute_rules
        .delete(project.id, rule_id)
        .await
        .map_err(error_response)?;

    if !removed {
        return Err(json_error(StatusCode::NOT_FOUND, "mute rule not found"));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Validate and normalize the request's tag pairs: trim, require non-empty
/// key+value, reject an empty set or duplicate keys (the latter could never
/// match under AND and collides with the storage primary key).
fn validate_tags(tags: Vec<TagMatch>) -> Result<Vec<TagMatch>, Response> {
    if tags.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "at least one tag is required",
        ));
    }

    let mut seen = BTreeSet::new();
    let mut cleaned = Vec::with_capacity(tags.len());
    for tag in tags {
        let key = tag.key.trim().to_string();
        let value = tag.value.trim().to_string();
        if key.is_empty() || value.is_empty() {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "tag key and value must not be empty",
            ));
        }
        if !seen.insert(key.clone()) {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                format!("duplicate tag key: {key}"),
            ));
        }
        cleaned.push(TagMatch { key, value });
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(k: &str, v: &str) -> TagMatch {
        TagMatch {
            key: k.to_string(),
            value: v.to_string(),
        }
    }

    #[test]
    fn validate_trims_and_keeps_pairs() {
        let out = validate_tags(vec![tag(" env ", " staging ")]).expect("valid");
        assert_eq!(out, vec![tag("env", "staging")]);
    }

    #[test]
    fn validate_rejects_empty_set() {
        assert!(validate_tags(vec![]).is_err());
    }

    #[test]
    fn validate_rejects_empty_key_or_value() {
        assert!(validate_tags(vec![tag("", "v")]).is_err());
        assert!(validate_tags(vec![tag("k", "  ")]).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_keys() {
        assert!(validate_tags(vec![tag("env", "a"), tag("env", "b")]).is_err());
    }
}
