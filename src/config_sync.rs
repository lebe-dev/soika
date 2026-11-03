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
fn apply_alertmanager_labels(
    config: &mut crate::domain::notifications::AlertmanagerConfig,
    value: &str,
) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in value.split(',') {
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            Error::validation(
                "ALERTMANAGER_DEFAULT_LABELS must contain comma-separated name=value pairs",
            )
        })?;
        let key = key.trim();
        if key.is_empty() || !seen.insert(key.to_string()) {
            return Err(Error::validation(
                "ALERTMANAGER_DEFAULT_LABELS contains an empty or duplicate name",
            ));
        }
        config
            .labels
            .insert(key.to_string(), value.trim().to_string());
    }
    Ok(())
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
    let env_file_exists = if settings.service_source == ManagementSource::Env {
        std::path::Path::new(&settings.path)
            .try_exists()
            .map_err(|_| {
                Error::validation(format!(
                    "configuration file unreadable at {}",
                    settings.path
                ))
            })?
    } else {
        false
    };
    if enabled || env_file_exists {
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
    if settings.service_source == ManagementSource::Env {
        let mut notifications = crate::domain::notifications::NotificationConfig::default();
        notifications.alertmanager.enabled = state.config.notifications_alertmanager_enabled;
        notifications.alertmanager.url = state.config.alertmanager_url.clone().unwrap_or_default();
        if let Ok(labels) = std::env::var("ALERTMANAGER_DEFAULT_LABELS") {
            apply_alertmanager_labels(&mut notifications.alertmanager, &labels)?;
        }
        notifications.validate()?;
        for labels in desired.alertmanager_project_labels.values() {
            let mut config = notifications.alertmanager.clone();
            config.labels.extend(labels.clone());
            config.validate()?;
        }
        state.notifications.set(&notifications).await?;
        state
            .settings
            .set_org_name(state.config.organization_name.clone())
            .await?;
        state
            .settings
            .set_allow_signup(state.config.allow_signup)
            .await?;
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
    if settings.service_source == ManagementSource::Env {
        state
            .settings
            .set_default_team_ids(settings.default_team_ids.clone())
            .await?;
    }
    report.default_file = default_file;
    state
        .alertmanager
        .set_project_labels(desired.alertmanager_project_labels)
        .await;
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

/// Invites are an opt-in feature (`ALLOW_INVITES`), independent of how teams are managed.
pub fn require_invites_enabled(state: &AppState) -> Result<()> {
    if !state.config.allow_invites {
        return Err(Error::Forbidden("invites are disabled".into()));
    }
    Ok(())
}

#[cfg(test)]
mod alertmanager_label_tests {
    use super::*;

    #[test]
    fn env_labels_extend_defaults_and_validate_templates() {
        let mut config = crate::domain::notifications::AlertmanagerConfig::default();
        apply_alertmanager_labels(&mut config, "team=platform, severity={{ level }}, url=a=b")
            .unwrap();
        assert_eq!(config.labels["alertname"], "SoikaIssue");
        assert_eq!(config.labels["team"], "platform");
        assert_eq!(config.labels["severity"], "{{ level }}");
        assert_eq!(config.labels["url"], "a=b");
        config.validate().unwrap();
    }

    #[test]
    fn env_labels_reject_malformed_duplicates_and_invalid_values() {
        for value in [
            "team",
            "=value",
            "team=a,team=b",
            "team=a,",
            "issue_id=x",
            "team={{ secret }}",
        ] {
            let mut config = crate::domain::notifications::AlertmanagerConfig::default();
            assert!(
                apply_alertmanager_labels(&mut config, value)
                    .and_then(|()| config.validate())
                    .is_err(),
                "{value}"
            );
        }
    }
}
