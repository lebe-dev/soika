//! Notification configuration and strict, scalar-only template substitution.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VARIABLES: &[&str] = &[
    "project_name",
    "project_slug",
    "project_id",
    "team_id",
    "issue_id",
    "issue_short_id",
    "title",
    "culprit",
    "level",
    "environment",
    "release",
    "event_count",
    "first_seen",
    "last_seen",
    "issue_url",
    "org_name",
    "trigger",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationConfig {
    pub alertmanager: AlertmanagerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AlertmanagerConfig {
    pub enabled: bool,
    /// Alertmanager base URL, optionally including a reverse-proxy path prefix.
    pub url: String,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
}
impl Default for AlertmanagerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            labels: BTreeMap::from([
                ("alertname".into(), "SoikaIssue".into()),
                ("project".into(), "{{ project_slug }}".into()),
                ("severity".into(), "{{ level }}".into()),
            ]),
            annotations: BTreeMap::from([
                ("summary".into(), "{{ title }}".into()),
                ("description".into(), "{{ culprit }}".into()),
                ("issue_url".into(), "{{ issue_url }}".into()),
                ("trigger".into(), "{{ trigger }}".into()),
                ("environment".into(), "{{ environment }}".into()),
                ("release".into(), "{{ release }}".into()),
                ("event_count".into(), "{{ event_count }}".into()),
            ]),
        }
    }
}
impl NotificationConfig {
    /// Validate persisted templates and any explicit URL. The effective URL can
    /// come from runtime configuration and is checked before enabling delivery.
    pub fn validate_stored(&self) -> Result<()> {
        let mut config = self.alertmanager.clone();
        config.enabled = false;
        config.validate()
    }
    pub fn effective(&self, default_url: Option<&str>) -> Result<Self> {
        let mut config = self.clone();
        if config.alertmanager.url.trim().is_empty() {
            config.alertmanager.url = default_url.unwrap_or_default().to_string();
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.alertmanager.validate()
    }
}
impl AlertmanagerConfig {
    pub fn validate(&self) -> Result<()> {
        if self.enabled || !self.url.is_empty() {
            let url = reqwest::Url::parse(&self.url)
                .map_err(|_| Error::validation("invalid Alertmanager base URL"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(Error::validation(
                    "Alertmanager requires an HTTP(S) base URL without credentials, query or fragment",
                ));
            }
        }
        let variables = sample_variables();
        for map in [&self.labels, &self.annotations] {
            if map.len() > 64 {
                return Err(Error::validation(
                    "at most 64 labels or annotations are allowed",
                ));
            }
            for (key, value) in map {
                let mut chars = key.chars();
                if !chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                    || key.len() > 128
                {
                    return Err(Error::validation("invalid label or annotation name"));
                }
                if value.len() > 8192 {
                    return Err(Error::validation("notification template is too long"));
                }
                render_template(value, &variables)?;
            }
        }
        if self.labels.contains_key("issue_id") || self.labels.contains_key("project_id") {
            return Err(Error::validation(
                "issue_id and project_id labels are reserved",
            ));
        }
        if self
            .labels
            .get("alertname")
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(Error::validation("alertname label is required"));
        }
        Ok(())
    }
}

/// No evaluation, loops, filters, escaping or recursive expansion. JSON encoding
/// happens after rendering, so issue data cannot inject additional payload fields.
pub fn render_template(template: &str, variables: &BTreeMap<String, String>) -> Result<String> {
    if template.contains("{%") || template.contains("{#") {
        return Err(Error::validation(
            "only {{ variable }} placeholders are supported",
        ));
    }
    let mut out = String::new();
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        let prefix = &remaining[..start];
        if prefix.contains("}}") {
            return Err(Error::validation("invalid notification placeholder"));
        }
        out.push_str(prefix);
        let tail = &remaining[start + 2..];
        let end = tail
            .find("}}")
            .ok_or_else(|| Error::validation("unclosed notification placeholder"))?;
        let name = tail[..end].trim();
        if !VARIABLES.contains(&name) {
            return Err(Error::validation(format!(
                "unknown notification variable: {name}"
            )));
        }
        out.push_str(
            variables
                .get(name)
                .ok_or_else(|| Error::validation("missing notification variable"))?,
        );
        remaining = &tail[end + 2..];
    }
    if remaining.contains("}}") {
        return Err(Error::validation("invalid notification placeholder"));
    }
    out.push_str(remaining);
    Ok(out)
}

pub fn sample_variables() -> BTreeMap<String, String> {
    VARIABLES
        .iter()
        .map(|key| {
            (
                (*key).into(),
                match *key {
                    "project_name" => "Example project",
                    "project_slug" => "example",
                    "project_id" => "00000000-0000-0000-0000-000000000001",
                    "team_id" => "00000000-0000-0000-0000-000000000002",
                    "issue_id" => "00000000-0000-0000-0000-000000000003",
                    "issue_short_id" => "iss001",
                    "title" => "ValueError: example error",
                    "culprit" => "app.checkout",
                    "level" => "error",
                    "environment" => "production",
                    "release" => "1.0.0",
                    "event_count" => "42",
                    "first_seen" | "last_seen" => "2026-01-01T12:00:00Z",
                    "issue_url" => "https://errors.example.com/projects/web001/issues/iss001",
                    "org_name" => "soika",
                    "trigger" => "new_issue",
                    _ => "",
                }
                .into(),
            )
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertmanagerAlert {
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub starts_at: super::Timestamp,
    pub ends_at: super::Timestamp,
    #[serde(rename = "generatorURL")]
    pub generator_url: String,
}

/// Saved before HTTP delivery, retaining the exact identity across retries,
/// restarts, template edits, resolution and deletion of the originating issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveAlert {
    pub issue_id: super::Id,
    pub endpoint: String,
    pub trigger: String,
    pub alert: AlertmanagerAlert,
}
