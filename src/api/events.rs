//! Event API handlers (MVP §16 / §6 stacktrace viewer).
//!
//! Session-cookie auth; access scoped to the project the event belongs to.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::api::issues::EventView;
use crate::api::projects::{error_response, json_error, parse_id, require_member, CurrentUser};
use crate::state::AppState;

/// `GET /events/{id}` — single event detail (full payload / stacktrace, §6).
pub async fn get(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let event_id = match parse_id(&id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let event = match state.events.find_by_id(event_id).await {
        Ok(Some(event)) => event,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "event not found"),
        Err(err) => return error_response(err),
    };

    if let Err(resp) = require_member(&state, &user, event.project_id).await {
        return resp;
    }

    Json(EventView::from(event)).into_response()
}
