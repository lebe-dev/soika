//! Service settings & admin API handlers.
//!
//! Instance-wide settings managed by the built-in/instance admin: the
//! `allow_signup` toggle (persisted to the DB, mirrors `ALLOW_SIGNUP`) and the
//! organization display name. SMTP is config-only in the MVP and surfaced
//! READ-ONLY here. All endpoints require an instance admin.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::api::teams::{AdminUser, ApiError};
use crate::domain::{Id, ServiceSettings, Timestamp, User};
use crate::error::Error;
use crate::state::AppState;

/// Full settings view returned to the admin UI.
#[derive(Debug, Serialize)]
pub struct SettingsView {
    /// Public self-registration toggle (persisted; mirrors `ALLOW_SIGNUP`).
    pub allow_signup: bool,
    /// Organization display name.
    pub org_name: String,
    pub updated_at: Timestamp,
    /// SMTP configuration, surfaced READ-ONLY.
    pub smtp: SmtpStatus,
}

/// Read-only SMTP status derived from runtime config. Secrets are never echoed.
#[derive(Debug, Serialize)]
pub struct SmtpStatus {
    /// Whether SMTP is configured (`SMTP_HOST` set). When false, email features
    /// degrade to UI-only.
    pub configured: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub from: Option<String>,
}

/// `PATCH /settings` body. Absent fields are left unchanged.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateSettings {
    pub allow_signup: Option<bool>,
    pub org_name: Option<String>,
}

/// A client-safe user row for the admin users list.
#[derive(Debug, Serialize)]
pub struct AdminUserRow {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    pub notifications_enabled: bool,
    pub created_at: Timestamp,
}

impl From<User> for AdminUserRow {
    fn from(u: User) -> Self {
        AdminUserRow {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            is_admin: u.is_admin,
            notifications_enabled: u.notifications_enabled,
            created_at: u.created_at,
        }
    }
}

/// Build the read-only SMTP status from runtime [`crate::config::Config`].
fn smtp_status(state: &AppState) -> SmtpStatus {
    match &state.config.smtp {
        Some(smtp) => SmtpStatus {
            configured: true,
            host: Some(smtp.host.clone()),
            port: Some(smtp.port),
            from: smtp.from.clone(),
        },
        None => SmtpStatus {
            configured: false,
            host: None,
            port: None,
            from: None,
        },
    }
}

fn to_view(settings: ServiceSettings, state: &AppState) -> SettingsView {
    SettingsView {
        allow_signup: settings.allow_signup,
        org_name: settings.org_name,
        updated_at: settings.updated_at,
        smtp: smtp_status(state),
    }
}

/// `GET /settings` — read instance-wide settings (admin).
pub async fn get(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<SettingsView>, ApiError> {
    let settings = state.settings.get().await?;
    Ok(Json(to_view(settings, &state)))
}

/// `PATCH /settings` — update `allow_signup` and/or `org_name` (admin).
///
/// SMTP fields are intentionally not writable here.
pub async fn update(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateSettings>,
) -> Result<Json<SettingsView>, ApiError> {
    if body.allow_signup.is_none() && body.org_name.is_none() {
        return Err(ApiError(Error::validation("no settings fields provided")));
    }

    // Apply org name first (it can fail validation), then the toggle.
    if let Some(name) = body.org_name {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApiError(Error::validation(
                "organization name must not be empty",
            )));
        }
        state.settings.set_org_name(trimmed.to_string()).await?;
    }

    if let Some(allow) = body.allow_signup {
        state.settings.set_allow_signup(allow).await?;
    }

    let settings = state.settings.get().await?;
    Ok(Json(to_view(settings, &state)))
}

/// `GET /admin/users` — list all users (instance admin).
pub async fn list_users(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminUserRow>>, ApiError> {
    let users = state.users.list().await?;
    Ok(Json(users.into_iter().map(AdminUserRow::from).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_user_row_omits_password_hash() {
        let row = AdminUserRow {
            id: Id::nil(),
            email: "a@b.c".into(),
            display_name: "A".into(),
            is_admin: true,
            notifications_enabled: true,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(!json.contains("password"));
        assert!(json.contains("\"is_admin\":true"));
    }

    #[test]
    fn smtp_status_unconfigured_serializes_nulls() {
        let status = SmtpStatus {
            configured: false,
            host: None,
            port: None,
            from: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"configured\":false"));
        assert!(json.contains("\"host\":null"));
    }

    #[test]
    fn settings_view_includes_read_only_smtp() {
        let view = SettingsView {
            allow_signup: true,
            org_name: "Acme".into(),
            updated_at: chrono::Utc::now(),
            smtp: SmtpStatus {
                configured: true,
                host: Some("smtp.example.com".into()),
                port: Some(587),
                from: Some("noreply@example.com".into()),
            },
        };
        let json = serde_json::to_string(&view).unwrap();
        // Secrets (username/password) are never part of the shape.
        assert!(!json.contains("password"));
        assert!(!json.contains("username"));
        assert!(json.contains("\"allow_signup\":true"));
        assert!(json.contains("smtp.example.com"));
    }
}
