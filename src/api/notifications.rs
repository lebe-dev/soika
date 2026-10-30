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
    pub default_url: Option<String>,
    pub config: NotificationConfig,
    pub variables: &'static [&'static str],
}
pub async fn get(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<NotificationView>, ApiError> {
    Ok(Json(NotificationView {
        source: state.config.management.notification_source,
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
    if state.config.management.notification_source == ManagementSource::Yaml {
        return Err(
            Error::Forbidden("notifications are managed by the configuration file".into()).into(),
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
    let config = config.effective(state.config.alertmanager_url.as_deref())?;
    Ok(Json(crate::alertmanager::render_alert(
        &config.alertmanager,
        &sample_variables(),
        chrono::Utc::now(),
    )?))
}
