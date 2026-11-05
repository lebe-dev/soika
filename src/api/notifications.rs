//! Instance notification administration; all routes require an instance manager.
use crate::{
    AppState, Error,
    api::teams::{AdminUser, ApiError},
    domain::{
        config_file::ManagementSource,
        notifications::{NotificationConfig, VARIABLES, sample_variables},
    },
};
use axum::{Json, extract::State};
use serde::Serialize;

#[derive(Serialize)]
pub struct NotificationView {
    pub source: ManagementSource,
    pub interval_source: ManagementSource,
    pub interval: crate::domain::notifications::AlertmanagerInterval,
    pub feature_enabled: bool,
    pub alertmanager_skip_tls: bool,
    pub default_url: Option<String>,
    pub config: NotificationConfig,
    pub variables: &'static [&'static str],
}
pub async fn get(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<NotificationView>, ApiError> {
    Ok(Json(NotificationView {
        interval_source: state.config.management.project_source,
        interval: state.notifications.interval(None).await?,
        source: if state.config.management.service_source == ManagementSource::Env {
            ManagementSource::Env
        } else {
            state.config.management.notification_source
        },
        feature_enabled: state.config.notifications_alertmanager_enabled,
        alertmanager_skip_tls: state.config.alertmanager_skip_tls,
        default_url: state.config.alertmanager_url.clone(),
        config: state.notifications.get().await?,
        variables: VARIABLES,
    }))
}
pub async fn update(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(config): Json<NotificationConfig>,
) -> Result<Json<NotificationView>, ApiError> {
    require_feature(&state)?;
    if state.config.management.service_source == ManagementSource::Env
        || state.config.management.notification_source != ManagementSource::Ui
    {
        return Err(
            Error::Forbidden("notifications are managed by external configuration".into()).into(),
        );
    }
    config.effective(state.config.alertmanager_url.as_deref())?;
    state.notifications.set(&config).await?;
    tracing::info!(actor_id = %admin.0.id, enabled = config.alertmanager.enabled, "notification settings updated");
    get(admin, State(state)).await
}
pub async fn preview(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(config): Json<NotificationConfig>,
) -> Result<Json<crate::domain::notifications::AlertmanagerAlert>, ApiError> {
    require_feature(&state)?;
    let config = config.effective(state.config.alertmanager_url.as_deref())?;
    Ok(Json(crate::alertmanager::render_alert(
        &config.alertmanager,
        &sample_variables(),
        chrono::Utc::now(),
    )?))
}

fn require_feature(state: &AppState) -> Result<(), ApiError> {
    if !state.config.notifications_alertmanager_enabled {
        return Err(Error::Forbidden(
            "Alertmanager is disabled by NOTIFICATIONS_ALERTMANAGER_ENABLED".into(),
        )
        .into());
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct TestAlertRequest {
    pub locale: crate::domain::Locale,
}

pub async fn test_alert(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(request): Json<TestAlertRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_feature(&state)?;
    if state.config.management.service_source == ManagementSource::Ui
        && state.config.management.notification_source == ManagementSource::Ui
        && state
            .notifications
            .get()
            .await?
            .alertmanager
            .url
            .trim()
            .is_empty()
    {
        return Err(
            Error::validation("save the Alertmanager URL before sending a test alert").into(),
        );
    }
    crate::alertmanager::send_test(&state, request.locale).await?;
    tracing::info!(actor_id = %admin.0.id, channel = "alertmanager", "test notification delivered");
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntervalRequest {
    pub interval_seconds: Option<i64>,
}

pub async fn update_interval(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<IntervalRequest>,
) -> Result<Json<crate::domain::notifications::AlertmanagerInterval>, ApiError> {
    crate::config_sync::require_ui_management(&state, false)?;
    state
        .notifications
        .set_interval(None, body.interval_seconds)
        .await?;
    Ok(Json(state.notifications.interval(None).await?))
}

pub async fn project_interval(
    crate::api::projects::CurrentUser(user): crate::api::projects::CurrentUser,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let project = match crate::api::projects::resolve_project(&state, &id).await {
        Ok(project) => project,
        Err(response) => return response,
    };
    if let Err(response) = crate::api::projects::require_member(&state, &user, project.id).await {
        return response;
    }
    match state.notifications.interval(Some(project.id)).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => ApiError(error).into_response(),
    }
}

pub async fn update_project_interval(
    crate::api::projects::CurrentUser(user): crate::api::projects::CurrentUser,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<IntervalRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(error) = crate::config_sync::require_ui_management(&state, false) {
        return ApiError(error).into_response();
    }
    let project = match crate::api::projects::resolve_project(&state, &id).await {
        Ok(project) => project,
        Err(response) => return response,
    };
    if let Err(response) = crate::api::projects::require_admin(&state, &user, project.id).await {
        return response;
    }
    if let Err(error) = state
        .notifications
        .set_interval(Some(project.id), body.interval_seconds)
        .await
    {
        return ApiError(error).into_response();
    }
    match state.notifications.interval(Some(project.id)).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => ApiError(error).into_response(),
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeverityRequest {
    pub severity: Option<crate::domain::notifications::AlertSeverity>,
}

pub async fn project_severity(
    crate::api::projects::CurrentUser(user): crate::api::projects::CurrentUser,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let project = match crate::api::projects::resolve_project(&state, &id).await {
        Ok(project) => project,
        Err(response) => return response,
    };
    if let Err(response) = crate::api::projects::require_member(&state, &user, project.id).await {
        return response;
    }
    match state.notifications.severity(project.id).await {
        Ok(severity) => Json(SeverityRequest { severity }).into_response(),
        Err(error) => ApiError(error).into_response(),
    }
}

pub async fn update_project_severity(
    crate::api::projects::CurrentUser(user): crate::api::projects::CurrentUser,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Result<Json<SeverityRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let body = match body {
        Ok(Json(body)) => body,
        Err(_) => {
            return ApiError(Error::validation(
                "severity must be critical, warning, info or null",
            ))
            .into_response();
        }
    };
    if let Err(error) = crate::config_sync::require_ui_management(&state, false) {
        return ApiError(error).into_response();
    }
    let project = match crate::api::projects::resolve_project(&state, &id).await {
        Ok(project) => project,
        Err(response) => return response,
    };
    if let Err(response) = crate::api::projects::require_admin(&state, &user, project.id).await {
        return response;
    }
    if let Err(error) = state
        .notifications
        .set_severity(project.id, body.severity)
        .await
    {
        return ApiError(error).into_response();
    }
    tracing::info!(actor_id = %user.id, project_id = %project.id, severity = ?body.severity, "project alert severity updated");
    Json(body).into_response()
}
