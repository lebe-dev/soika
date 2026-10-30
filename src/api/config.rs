//! Session-scoped SPA bootstrap. Resource reads remain available for filtering and refresh.
use crate::{
    api::{
        settings,
        teams::{AdminUser, ApiError, AuthUser, OptionalAuthUser},
    },
    auth,
    state::AppState,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Serialize)]
struct ConfigView {
    #[serde(flatten)]
    config: auth::AuthConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    admin: Option<AdminConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    passkeys: Option<Vec<auth::passkey_handlers::PasskeyView>>,
}

#[derive(Serialize)]
struct AdminConfig {
    notifications: crate::api::notifications::NotificationView,
    settings: settings::SettingsView,
    users: Vec<settings::AdminUserRow>,
}

pub async fn get(
    State(state): State<AppState>,
    OptionalAuthUser(user): OptionalAuthUser,
) -> Result<Response, ApiError> {
    let admin = match user
        .as_ref()
        .filter(|u| u.instance_role.can_manage_instance())
    {
        Some(user) => Some(AdminConfig {
            notifications: crate::api::notifications::get(
                AdminUser(user.clone()),
                State(state.clone()),
            )
            .await?
            .0,
            settings: settings::get(AdminUser(user.clone()), State(state.clone()))
                .await?
                .0,
            users: settings::list_users(AdminUser(user.clone()), State(state.clone()))
                .await?
                .0,
        }),
        None => None,
    };
    let passkeys = match user.as_ref().filter(|_| state.webauthn.is_some()) {
        Some(user) => Some(
            auth::passkey_list(AuthUser(user.clone()), State(state.clone()))
                .await?
                .0,
        ),
        None => None,
    };
    let config = auth::auth_config(State(state), OptionalAuthUser(user))
        .await
        .0;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Cookie"));
    Ok((
        headers,
        Json(ConfigView {
            config,
            admin,
            passkeys,
        }),
    )
        .into_response())
}
