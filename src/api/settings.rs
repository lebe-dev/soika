//! Service settings & admin API handlers.
//!
//! Instance-wide settings managed by the built-in/instance admin: the
//! `allow_signup` toggle (persisted to the DB, mirrors `ALLOW_SIGNUP`) and the
//! organization display name and default teams for new users. SMTP is config-only in the MVP and surfaced
//! READ-ONLY here. All endpoints require an instance admin.

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::api::teams::{AdminUser, ApiError};
use crate::domain::{Id, InstanceRole, ServiceSettings, Timestamp, User, UserStatus};
use crate::error::Error;
use crate::ports::UserUpdate;
use crate::state::AppState;

/// Full settings view returned to the admin UI.
#[derive(Debug, Serialize)]
pub struct SettingsView {
    pub management: ManagementStatus,
    /// Public self-registration toggle (persisted; mirrors `ALLOW_SIGNUP`).
    pub allow_signup: bool,
    /// Organization display name.
    pub org_name: String,
    /// Teams assigned as Contributors only when an account is created.
    pub default_team_ids: Vec<Id>,
    pub updated_at: Timestamp,
    /// SMTP configuration, surfaced READ-ONLY.
    pub smtp: SmtpStatus,
}

#[derive(Debug, Serialize)]
pub struct ManagementStatus {
    pub settings: crate::domain::config_file::ManagementSettings,
    pub report: crate::domain::config_file::SyncReport,
    pub oauth_enabled: bool,
}
/// Read-only SMTP status derived from runtime config. Secrets are never echoed.
#[derive(Debug, Serialize)]
pub struct SmtpStatus {
    /// Whether outgoing email delivery is enabled (`SMTP_ENABLED`).
    pub enabled: bool,
    /// Whether SMTP is configured (`SMTP_HOST` set), independently of `enabled`.
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
    pub default_team_ids: Option<Vec<Id>>,
}

/// A client-safe user row for the admin users list.
#[derive(Debug, Serialize)]
pub struct AdminUserRow {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    /// Stored instance-wide role (`owner | manager | member`), independent of ADMIN_USERS.
    pub instance_role: InstanceRole,
    pub notifications_enabled: bool,
    /// Activation status: `active` | `pending` (awaiting admin approval).
    pub status: UserStatus,
    pub created_at: Timestamp,
}

impl From<User> for AdminUserRow {
    fn from(u: User) -> Self {
        AdminUserRow {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            instance_role: u.instance_role,
            notifications_enabled: u.notifications_enabled,
            status: u.status,
            created_at: u.created_at,
        }
    }
}

/// Build the read-only SMTP status from runtime [`crate::config::Config`].
fn smtp_status(state: &AppState) -> SmtpStatus {
    match &state.config.smtp {
        Some(smtp) => SmtpStatus {
            enabled: state.config.smtp_enabled,
            configured: true,
            host: Some(smtp.host.clone()),
            port: Some(smtp.port),
            from: smtp.from.clone(),
        },
        None => SmtpStatus {
            enabled: state.config.smtp_enabled,
            configured: false,
            host: None,
            port: None,
            from: None,
        },
    }
}

fn to_view(settings: ServiceSettings, state: &AppState, default_team_ids: Vec<Id>) -> SettingsView {
    SettingsView {
        management: ManagementStatus {
            settings: state.config.management.clone(),
            report: (*state.sync_report).clone(),
            oauth_enabled: state.oidc.is_some(),
        },
        allow_signup: settings.allow_signup,
        org_name: settings.org_name,
        default_team_ids,
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
    Ok(Json(to_view(
        settings,
        &state,
        state.settings.default_team_ids().await?,
    )))
}

/// `PATCH /settings` — update sign-up, organization name and/or default teams (admin).
///
/// SMTP fields are intentionally not writable here.
pub async fn update(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateSettings>,
) -> Result<Json<SettingsView>, ApiError> {
    if body.allow_signup.is_some() || body.default_team_ids.is_some() {
        crate::config_sync::require_ui_management(&state, true)?;
    }
    if body.allow_signup.is_none() && body.org_name.is_none() && body.default_team_ids.is_none() {
        return Err(ApiError(Error::validation("no settings fields provided")));
    }

    // Validate the name before any settings are changed.
    if body
        .org_name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(ApiError(Error::validation(
            "organization name must not be empty",
        )));
    }
    // Default teams are validated and replaced atomically by the repository.
    if let Some(ids) = body.default_team_ids {
        state.settings.set_default_team_ids(ids).await?;
    }

    if let Some(name) = body.org_name {
        let trimmed = name.trim();
        state.settings.set_org_name(trimmed.to_string()).await?;
    }

    if let Some(allow) = body.allow_signup {
        state.settings.set_allow_signup(allow).await?;
    }

    let settings = state.settings.get().await?;
    Ok(Json(to_view(
        settings,
        &state,
        state.settings.default_team_ids().await?,
    )))
}

/// `POST /settings/test-email` body. An absent/empty `to` defaults to the
/// requesting admin's own address.
#[derive(Debug, Default, Deserialize)]
pub struct TestEmail {
    pub to: Option<String>,
}

/// Result of a successful test-email send: the address it was delivered to.
#[derive(Debug, Serialize)]
pub struct TestEmailResult {
    pub sent_to: String,
}

/// `POST /settings/test-email` — send a test message to verify SMTP (admin).
///
/// Returns `400` when SMTP is unconfigured (nothing to test) and surfaces any
/// SMTP/transport failure as a `500` carrying the underlying error, so the admin
/// can diagnose host/credential/From problems directly from the UI.
pub async fn test_email(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<TestEmail>,
) -> Result<Json<TestEmailResult>, ApiError> {
    if !state.mailer.is_enabled() {
        return Err(ApiError(Error::validation(
            "SMTP is not configured; set SMTP_HOST and related variables to enable email",
        )));
    }

    let to = match body.to.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => t.to_string(),
        Some(_) => return Err(ApiError(Error::validation("recipient must not be empty"))),
        None => admin.email.clone(),
    };

    let email = crate::mail::test_email(&state.config, &to);
    state.mailer.send(email).await?;
    tracing::info!("test email delivered");
    Ok(Json(TestEmailResult { sent_to: to }))
}

/// `GET /admin/users` — list all users (instance admin).
pub async fn list_users(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminUserRow>>, ApiError> {
    let users = state.users.list().await?;
    Ok(Json(users.into_iter().map(AdminUserRow::from).collect()))
}

/// Parse a path id segment into a domain [`Id`], mapping a bad UUID to a 400.
fn parse_user_id(raw: &str) -> Result<Id, Error> {
    Id::parse_str(raw).map_err(|_| Error::validation(format!("invalid user id: {raw}")))
}

/// `POST /admin/users/{id}/approve` — approve a pending account (instance admin).
///
/// Flips the account to `active`, letting it hold a session on the next sign-in.
/// Approving an already-active account is a harmless no-op. Returns the updated row.
pub async fn approve_user(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AdminUserRow>, ApiError> {
    let user_id = parse_user_id(&id)?;
    let user = state
        .users
        .update(
            user_id,
            UserUpdate {
                status: Some(UserStatus::Active),
                ..Default::default()
            },
        )
        .await?;
    tracing::info!(target_user_id = %user.id, "user approved");
    Ok(Json(AdminUserRow::from(user)))
}

/// `DELETE /admin/users/{id}` — reject/delete an account.
///
/// Gated by an instance manager ([`AdminUser`]). Refuses deleting one's own
/// account. Deleting an `Owner` requires the caller to be an `Owner` (Owner-only
/// destructive op) and is blocked when the target is the **last** Owner, so the
/// instance can never be left without an Owner via this endpoint.
pub async fn delete_user(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeletedUser>, ApiError> {
    let user_id = parse_user_id(&id)?;
    if user_id == admin.id {
        return Err(ApiError(Error::Forbidden(
            "you cannot delete your own account".into(),
        )));
    }

    let target = state
        .users
        .find_by_id(user_id)
        .await?
        .ok_or_else(|| Error::not_found(format!("user not found: {user_id}")))?;

    if state
        .config
        .admin_users
        .effective_role(&target.email, target.instance_role)
        .is_owner()
    {
        // Deleting an Owner is an Owner-only, destructive operation.
        if !admin.instance_role.is_owner() {
            return Err(ApiError(Error::Forbidden(
                "only an owner can delete an owner".into(),
            )));
        }
        // Never orphan the instance: keep at least one Owner.
        if target.instance_role.is_owner() && state.users.count_owners().await? <= 1 {
            return Err(ApiError(Error::Conflict(
                "cannot delete the last owner".into(),
            )));
        }
    }

    state.users.delete(user_id).await?;
    tracing::info!(target_user_id = %user_id, "user deleted");
    Ok(Json(DeletedUser { id: user_id }))
}

/// `PATCH /admin/users/{id}` body — change a user's instance role.
#[derive(Debug, Deserialize)]
pub struct SetUserRole {
    pub instance_role: InstanceRole,
}

/// `PATCH /admin/users/{id}` — change a user's instance role.
///
/// Gated by an instance manager ([`AdminUser`]). Granting **or** revoking the
/// `Owner` role is an Owner-only operation. Revoking the last Owner is blocked
/// (last-Owner protection) so the instance always retains at least one Owner.
pub async fn set_user_role(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetUserRole>,
) -> Result<Json<AdminUserRow>, ApiError> {
    let user_id = parse_user_id(&id)?;
    let target = state
        .users
        .find_by_id(user_id)
        .await?
        .ok_or_else(|| Error::not_found(format!("user not found: {user_id}")))?;

    let new_role = body.instance_role;
    let was_owner = target.instance_role.is_owner();
    let becomes_owner = new_role.is_owner();

    // Granting or revoking Owner is Owner-only.
    if (was_owner || becomes_owner || state.config.admin_users.contains(&target.email))
        && !admin.instance_role.is_owner()
    {
        return Err(ApiError(Error::Forbidden(
            "only an owner can grant or revoke the owner role".into(),
        )));
    }

    // Last-Owner protection: refuse to demote the only remaining Owner.
    if was_owner && !becomes_owner && state.users.count_owners().await? <= 1 {
        return Err(ApiError(Error::Conflict(
            "cannot demote the last owner".into(),
        )));
    }

    let updated = state.users.set_instance_role(user_id, new_role).await?;
    tracing::info!(target_user_id = %user_id, instance_role = ?new_role, "instance role changed");
    Ok(Json(AdminUserRow::from(updated)))
}

/// Response for a successful user deletion.
#[derive(Debug, Serialize)]
pub struct DeletedUser {
    pub id: Id,
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
            instance_role: InstanceRole::Owner,
            notifications_enabled: true,
            status: UserStatus::Active,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(!json.contains("password"));
        assert!(json.contains("\"instance_role\":\"owner\""));
        assert!(json.contains("\"status\":\"active\""));
    }

    #[test]
    fn smtp_status_unconfigured_serializes_nulls() {
        let status = SmtpStatus {
            enabled: false,
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
            management: ManagementStatus {
                settings: Default::default(),
                report: Default::default(),
                oauth_enabled: false,
            },
            allow_signup: true,
            org_name: "Acme".into(),
            default_team_ids: vec![],
            updated_at: chrono::Utc::now(),
            smtp: SmtpStatus {
                enabled: true,
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
