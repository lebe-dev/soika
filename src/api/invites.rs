//! Invite API handlers.
//!
//! A team **admin** (or instance manager/owner) generates an invite: a
//! high-entropy, server-validated, expiring token plus a `{BASE_URL}/invite/{token}`
//! link. The link is returned so it can be **emailed** (when SMTP is configured)
//! AND/OR copied manually — invites work even on instances without email.
//!
//! Invites are scoped to a **team**: accepting one adds the user to the team
//! with the invite's [`TeamRole`].

// Auth/error guards return `Result<T, Response>`; axum's `Response` is large,
// which is fine here (these are control-flow short-circuits, never hot paths).
#![allow(clippy::result_large_err)]

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Duration;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::api::projects::{CurrentUser, error_response, json_error};
use crate::domain::{Id, Invite, TeamRole};
use crate::ports::{NewInvite, OutboundEmail};
use crate::state::AppState;

/// Default invite token lifetime (expiring token).
const INVITE_TTL_DAYS: i64 = 7;

/// Number of random bytes backing an invite token (256-bit, unguessable).
const TOKEN_BYTES: usize = 32;

/// An invite as returned to the client, including the shareable link.
#[derive(Debug, Serialize)]
pub struct InviteView {
    pub token: String,
    /// Target team id the invite grants membership of.
    pub team_id: Id,
    pub role: TeamRole,
    pub email: Option<String>,
    /// Copyable / emailable acceptance link: `{BASE_URL}/invite/{token}`.
    pub link: String,
    pub created_at: crate::domain::Timestamp,
    pub expires_at: crate::domain::Timestamp,
    pub accepted_at: Option<crate::domain::Timestamp>,
    /// Whether the invite email was actually dispatched (false if SMTP off).
    #[serde(default)]
    pub email_sent: bool,
}

impl InviteView {
    fn from_invite(invite: Invite, base_url: &str, email_sent: bool) -> Self {
        let link = invite_link(base_url, &invite.token);
        InviteView {
            token: invite.token,
            team_id: invite.team_id,
            role: invite.role,
            email: invite.email,
            link,
            created_at: invite.created_at,
            expires_at: invite.expires_at,
            accepted_at: invite.accepted_at,
            email_sent,
        }
    }
}

/// Build the acceptance link for a token.
fn invite_link(base_url: &str, token: &str) -> String {
    format!("{}/invite/{}", base_url.trim_end_matches('/'), token)
}

/// Parse a team-id path segment, mapping a bad UUID to a `400`.
fn parse_team_id(raw: &str) -> std::result::Result<Id, Response> {
    Id::parse_str(raw).map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid team id"))
}

/// Require that `user` may manage `team_id`'s invites: an instance
/// `Owner | Manager`, or a `TeamRole::Admin` of that team.
async fn require_team_manager(
    state: &AppState,
    user: &crate::domain::User,
    team_id: Id,
) -> std::result::Result<(), Response> {
    if user.instance_role.can_manage_instance() {
        return Ok(());
    }
    match state.teams.member_role(team_id, user.id).await {
        Ok(Some(TeamRole::Admin)) => Ok(()),
        Ok(_) => Err(json_error(
            StatusCode::FORBIDDEN,
            "requires team admin (or instance manager/owner)",
        )),
        Err(err) => Err(error_response(err)),
    }
}

/// `GET /teams/{id}/invites` — list pending invites for a team (team admin).
pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(team_id): Path<String>,
) -> Response {
    let team_id = match parse_team_id(&team_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_team_manager(&state, &user, team_id).await {
        return resp;
    }

    match state.invites.list_for_team(team_id).await {
        Ok(invites) => {
            let views: Vec<InviteView> = invites
                .into_iter()
                .map(|i| InviteView::from_invite(i, &state.config.base_url, false))
                .collect();
            Json(views).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// Request body for creating an invite.
#[derive(Debug, Deserialize)]
pub struct CreateInviteRequest {
    /// Team role to grant on acceptance. Defaults to `contributor`.
    #[serde(default)]
    pub role: Option<TeamRole>,
    /// Optional target email (link still works without it).
    #[serde(default)]
    pub email: Option<String>,
}

/// `POST /teams/{id}/invites` — create a team invite (team admin).
///
/// Returns the invite with its copyable link. If SMTP is configured and a
/// target email is given, the link is also emailed (best-effort; failure to
/// send does not fail the request — the link remains usable).
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(team_id): Path<String>,
    body: Option<Json<CreateInviteRequest>>,
) -> Response {
    let team_id = match parse_team_id(&team_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_team_manager(&state, &user, team_id).await {
        return resp;
    }

    // Surface a clear 404 for an unknown team rather than an opaque FK failure.
    match state.teams.find_by_id(team_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "team not found"),
        Err(err) => return error_response(err),
    }

    let req = body.map(|Json(b)| b).unwrap_or(CreateInviteRequest {
        role: None,
        email: None,
    });

    let role = req.role.unwrap_or(TeamRole::Contributor);
    let email = req
        .email
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty());

    if let Some(addr) = &email
        && !looks_like_email(addr)
    {
        return json_error(StatusCode::BAD_REQUEST, "invalid email address");
    }

    let expires_at = state.clock.now() + Duration::days(INVITE_TTL_DAYS);
    let token = generate_token();

    let new = NewInvite {
        token,
        team_id,
        role,
        email: email.clone(),
        created_by: Some(user.id),
        expires_at,
    };

    let invite = match state.invites.create(new).await {
        Ok(invite) => invite,
        Err(err) => return error_response(err),
    };

    // Best-effort email delivery; link is always returned regardless.
    let link = invite_link(&state.config.base_url, &invite.token);
    let mut email_sent = false;
    if let Some(addr) = &email
        && state.mailer.is_enabled()
    {
        let message = OutboundEmail {
            to: addr.clone(),
            subject: format!(
                "You've been invited to {} on soika",
                state.config.organization_name
            ),
            body: format!(
                "You have been invited to join a team on soika.\n\n\
                 Accept your invite:\n{link}\n\n\
                 This link expires in {INVITE_TTL_DAYS} days."
            ),
        };
        // Swallow send errors: the copyable link still works.
        email_sent = state.mailer.send(message).await.is_ok();
    }

    let view = InviteView::from_invite(invite, &state.config.base_url, email_sent);
    (StatusCode::CREATED, Json(view)).into_response()
}

/// `DELETE /teams/{id}/invites/{token}` — revoke a pending invite (team admin).
pub async fn revoke(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((team_id, token)): Path<(String, String)>,
) -> Response {
    let team_id = match parse_team_id(&team_id) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if let Err(resp) = require_team_manager(&state, &user, team_id).await {
        return resp;
    }

    // Verify the invite belongs to this team before deleting (avoid letting an
    // admin of team A delete an invite of team B by token).
    match state.invites.find_by_token(&token).await {
        Ok(Some(invite)) if invite.team_id == team_id => {}
        Ok(_) => return json_error(StatusCode::NOT_FOUND, "invite not found"),
        Err(err) => return error_response(err),
    }

    match state.invites.delete(&token).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => error_response(err),
    }
}

/// Generate a high-entropy, URL-safe invite token (256-bit, unguessable).
fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Minimal email sanity check (presence of a single `@` with non-empty parts).
fn looks_like_email(addr: &str) -> bool {
    let mut parts = addr.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_link_is_well_formed() {
        assert_eq!(
            invite_link("https://soika.example.com/", "abc"),
            "https://soika.example.com/invite/abc"
        );
        assert_eq!(
            invite_link("http://localhost:8080", "xyz"),
            "http://localhost:8080/invite/xyz"
        );
    }

    #[test]
    fn generate_token_is_url_safe_and_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        // URL_SAFE_NO_PAD of 32 bytes => 43 chars, no padding, no '/' or '+'.
        assert_eq!(a.len(), 43);
        assert!(!a.contains('=') && !a.contains('/') && !a.contains('+'));
    }

    #[test]
    fn email_validation() {
        assert!(looks_like_email("user@example.com"));
        assert!(looks_like_email("a.b+c@sub.domain.io"));
        assert!(!looks_like_email("no-at-sign"));
        assert!(!looks_like_email("@example.com"));
        assert!(!looks_like_email("user@"));
        assert!(!looks_like_email("user@nodot"));
        assert!(!looks_like_email("user@@x.com"));
        assert!(!looks_like_email("user@.com"));
        assert!(!looks_like_email("user@com."));
    }
}
