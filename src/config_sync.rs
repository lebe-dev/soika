//! Startup file loading. Values are expanded after YAML parsing to prevent YAML injection.
use crate::{AppState, Error, Result, domain::config_file::*};
use std::sync::Arc;
pub const DEFAULT_FILE: &str = "version: 1\nprojects: []\nteams: []\nnotifications: {}\n";
fn expand(value: &str) -> Result<String> {
    let mut out = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find("${") {
        out.push_str(&remaining[..start]);
        let tail = &remaining[start + 2..];
        let end = tail
            .find('}')
            .ok_or_else(|| Error::validation("unterminated environment variable reference"))?;
        let key = &tail[..end];
        out.push_str(
            &std::env::var(key)
                .map_err(|_| Error::validation(format!("environment variable {key} is not set")))?,
        );
        remaining = &tail[end + 1..];
    }
    out.push_str(remaining);
    Ok(out)
}
fn visit(value: &mut serde_yaml_ng::Value) -> Result<()> {
    match value {
        serde_yaml_ng::Value::String(s) => *s = expand(s)?,
        serde_yaml_ng::Value::Sequence(seq) => {
            for v in seq {
                visit(v)?;
            }
        }
        serde_yaml_ng::Value::Mapping(map) => {
            for (_, v) in map {
                visit(v)?;
            }
        }
        _ => {}
    };
    Ok(())
}
fn read_configuration(settings: &ManagementSettings) -> Result<(ConfigFile, bool)> {
    let text = std::fs::read_to_string(&settings.path).map_err(|_| {
        Error::validation(format!(
            "configuration file not found or unreadable at {}",
            settings.path
        ))
    })?;
    let default_file = text == DEFAULT_FILE;
    if default_file {
        tracing::warn!("using the default configuration file shipped with the image");
    }
    // Parse to generic YAML then expand only scalar strings, keeping secrets out of errors.
    let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)
        .map_err(|_| Error::validation("invalid configuration YAML"))?;
    discard_ui_sections(&mut value, settings);
    visit(&mut value)?;
    let expanded = serde_yaml_ng::to_string(&value)
        .map_err(|_| Error::validation("invalid configuration YAML"))?;
    let desired = parse(&expanded, settings)?;
    Ok((desired, default_file))
}
pub async fn synchronize(mut state: AppState) -> Result<AppState> {
    let settings = &state.config.management;
    let enabled = settings.project_source == ManagementSource::Yaml
        || settings.team_source == ManagementSource::Yaml
        || settings.notification_source == ManagementSource::Yaml;
    let mut desired = ConfigFile {
        version: 1,
        ..Default::default()
    };
    let mut default_file = false;
    if enabled {
        (desired, default_file) = read_configuration(settings)?;
        let has_groups = desired
            .teams
            .iter()
            .flatten()
            .any(|t| t.sso_groups.as_ref().is_some_and(|v| !v.is_empty()));
        if has_groups
            && !state
                .config
                .oidc
                .as_ref()
                .is_some_and(|o| o.kind == crate::config::OAuthProviderKind::Oidc)
        {
            return Err(Error::validation(
                "sso_groups require an OIDC provider that returns group claims",
            ));
        }
        if settings.team_source == ManagementSource::Yaml && state.config.oidc.is_none() {
            tracing::warn!(
                "teams are managed by the configuration file but OAuth is disabled: new users cannot join"
            );
        }
    }
    let notifications = match &desired.notifications {
        Some(config) => config.clone(),
        None => state.notifications.get().await?,
    };
    if state.config.notifications_alertmanager_enabled {
        notifications.effective(state.config.alertmanager_url.as_deref())?;
    }
    let mut report = state
        .config_sync
        .apply(&desired, settings, state.config.default_events_retention)
        .await?;
    report.default_file = default_file;
    for action in &report.actions {
        tracing::info!(entity = %action.entity, key = %action.key, action = ?action.action, "configuration action applied");
    }
    tracing::info!(
        created = report.created,
        updated = report.updated,
        unchanged = report.unchanged,
        restored = report.restored,
        orphaned = report.orphaned,
        deleted = report.deleted,
        "configuration sync finished"
    );
    state.sync_report = Arc::new(report);
    Ok(state)
}

pub fn require_ui_management(state: &AppState, team: bool) -> Result<()> {
    let (source, kind) = if team {
        (state.config.management.team_source, "teams")
    } else {
        (state.config.management.project_source, "projects")
    };
    if source == ManagementSource::Yaml {
        return Err(Error::Forbidden(format!(
            "{kind} are managed by the configuration file"
        )));
    }
    Ok(())
}
