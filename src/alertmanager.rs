//! Alertmanager lifecycle: durable delivery identity and periodic reconciliation.
use crate::{
    AppState, Error, Result,
    domain::{Issue, IssueStatus, Project, Timestamp, notifications::*},
};
use std::{collections::BTreeMap, time::Duration};

/// Alerts are refreshed every minute, with enough overlap for transient failures.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const ALERT_TTL: i64 = 300;

pub struct Alertmanager {
    client: std::sync::Arc<dyn crate::ports::AlertmanagerClient>,
    // Serialize initial sends and reconciliation within the single process.
    lock: tokio::sync::Mutex<()>,
}
impl Alertmanager {
    pub fn new(client: std::sync::Arc<dyn crate::ports::AlertmanagerClient>) -> Self {
        Self {
            client,
            lock: tokio::sync::Mutex::new(()),
        }
    }
    async fn send(&self, endpoint: &str, alert: &AlertmanagerAlert) -> Result<()> {
        self.client.send(endpoint, alert).await
    }
}

pub fn endpoint(config: &AlertmanagerConfig) -> String {
    format!("{}/api/v2/alerts", config.url.trim_end_matches('/'))
}

pub fn variables(
    state: &AppState,
    org: &str,
    project: &Project,
    issue: &Issue,
    trigger: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("project_name".into(), project.name.clone()),
        ("project_slug".into(), project.slug.clone()),
        ("project_id".into(), project.id.to_string()),
        ("team_id".into(), project.team_id.to_string()),
        ("issue_id".into(), issue.id.to_string()),
        ("issue_short_id".into(), issue.short_id.clone()),
        ("title".into(), issue.title.clone()),
        ("culprit".into(), issue.culprit.clone().unwrap_or_default()),
        (
            "level".into(),
            issue.level.clone().unwrap_or_else(|| "error".into()),
        ),
        (
            "environment".into(),
            issue.environment.clone().unwrap_or_default(),
        ),
        ("release".into(), issue.release.clone().unwrap_or_default()),
        ("event_count".into(), issue.event_count.to_string()),
        ("first_seen".into(), issue.first_seen.to_rfc3339()),
        ("last_seen".into(), issue.last_seen.to_rfc3339()),
        (
            "issue_url".into(),
            format!(
                "{}/projects/{}/issues/{}",
                state.config.base_url.trim_end_matches('/'),
                project.short_id,
                issue.short_id
            ),
        ),
        ("org_name".into(), org.into()),
        ("trigger".into(), trigger.into()),
    ])
}

pub fn render_alert(
    config: &AlertmanagerConfig,
    vars: &BTreeMap<String, String>,
    now: Timestamp,
) -> Result<AlertmanagerAlert> {
    let render = |map: &BTreeMap<String, String>| -> Result<BTreeMap<String, String>> {
        map.iter()
            .map(|(key, value)| Ok((key.clone(), render_template(value, vars)?)))
            .collect()
    };
    let required = |key: &str| {
        vars.get(key)
            .cloned()
            .ok_or_else(|| Error::validation(format!("missing notification variable: {key}")))
    };
    let mut labels = render(&config.labels)?;
    labels.insert("issue_id".into(), required("issue_id")?);
    labels.insert("project_id".into(), required("project_id")?);
    if labels.get("alertname").is_none_or(|s| s.trim().is_empty()) {
        return Err(Error::validation("rendered alertname must not be empty"));
    }
    Ok(AlertmanagerAlert {
        labels,
        annotations: render(&config.annotations)?,
        starts_at: now,
        ends_at: now + chrono::Duration::seconds(ALERT_TTL),
        generator_url: required("issue_url")?,
    })
}

/// Called only for a new issue or regression, after the usual mute checks.
pub async fn notify(
    state: &AppState,
    project: &Project,
    issue: &Issue,
    trigger: &str,
) -> Result<()> {
    if !state.config.notifications_alertmanager_enabled {
        return Ok(());
    }
    let _guard = state.alertmanager.lock.lock().await;
    let config = state
        .notifications
        .get()
        .await?
        .effective(state.config.alertmanager_url.as_deref())?
        .alertmanager;
    if !config.enabled {
        return Ok(());
    }
    let org = state.settings.get().await?.org_name;
    let alert = render_alert(
        &config,
        &variables(state, &org, project, issue, trigger),
        state.clock.now(),
    )?;
    let pending = ActiveAlert {
        issue_id: issue.id,
        endpoint: endpoint(&config),
        trigger: trigger.into(),
        alert,
    };
    // Persist before HTTP: a failed delivery is retried by reconciliation.
    // If a regression arrives before the old resolution sweep, close the old
    // identity before replacing it, even when the templates have changed.
    let old = state.notifications.find_alert(issue.id).await?;
    if let Some(old) = old {
        resolve(state, &old).await?;
    }
    state.notifications.save_alert(&pending).await?;
    state
        .alertmanager
        .send(&pending.endpoint, &pending.alert)
        .await?;
    tracing::info!(issue_id = %issue.id, project_id = %project.id, channel = "alertmanager", "notification delivered");
    Ok(())
}

async fn resolve(state: &AppState, active: &ActiveAlert) -> Result<()> {
    let mut alert = active.alert.clone();
    alert.ends_at = state.clock.now();
    state.alertmanager.send(&active.endpoint, &alert).await
}

/// Reconcile only tracked alerts, so enabling the channel does not page on old issues.
pub async fn refresh(state: &AppState) -> Result<()> {
    if !state.config.notifications_alertmanager_enabled {
        return Ok(());
    }
    let config = state
        .notifications
        .get()
        .await?
        .effective(state.config.alertmanager_url.as_deref())?
        .alertmanager;
    let org = state.settings.get().await?.org_name;
    let mut after = String::new();
    loop {
        let active = state.notifications.active(&after, 100).await?;
        if active.is_empty() {
            break;
        }
        for entry in active {
            after = entry.issue_id.to_string();
            let _guard = state.alertmanager.lock.lock().await;
            let Some(mut saved) = state.notifications.find_alert(entry.issue_id).await? else {
                continue;
            };
            if let Err(error) = reconcile(state, &config, &org, &mut saved).await {
                tracing::warn!(issue_id = %saved.issue_id, error = %error, channel = "alertmanager", "alert reconciliation failed");
            }
        }
    }
    Ok(())
}
async fn reconcile(
    state: &AppState,
    config: &AlertmanagerConfig,
    org: &str,
    saved: &mut ActiveAlert,
) -> Result<()> {
    let issue = state.issues.find_by_id(saved.issue_id).await?;
    let project = match &issue {
        Some(issue) => state.projects.find_by_id(issue.project_id).await?,
        None => None,
    };
    let live = match (&project, &issue) {
        (Some(p), Some(i)) => {
            config.enabled
                && !p.muted
                && p.orphaned_at.is_none()
                && i.status == IssueStatus::Unresolved
        }
        _ => false,
    };
    if !live {
        resolve(state, saved).await?;
        state.notifications.remove_alert(saved.issue_id).await?;
        return Ok(());
    }
    let (Some(project), Some(issue)) = (project, issue) else {
        return Err(Error::Internal("live alert has no issue or project".into()));
    };
    let mut alert = render_alert(
        config,
        &variables(state, org, &project, &issue, &saved.trigger),
        state.clock.now(),
    )?;
    if saved.endpoint != endpoint(config) || saved.alert.labels != alert.labels {
        resolve(state, saved).await?;
    } else {
        alert.starts_at = saved.alert.starts_at;
    }
    saved.endpoint = endpoint(config);
    saved.alert = alert;
    state.notifications.save_alert(saved).await?;
    state.alertmanager.send(&saved.endpoint, &saved.alert).await
}
