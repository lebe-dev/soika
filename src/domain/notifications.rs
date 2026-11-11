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

/// Service-owned names for issue alerts and the project test button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertmanagerNames {
    pub critical: String,
    pub warning: String,
    pub info: String,
    pub project_test: String,
}
impl Default for AlertmanagerNames {
    fn default() -> Self {
        Self {
            critical: AlertSeverity::Critical.alert_name().into(),
            warning: AlertSeverity::Warning.alert_name().into(),
            info: AlertSeverity::Info.alert_name().into(),
            project_test: "SoikaIssueCritical".into(),
        }
    }
}
impl AlertmanagerNames {
    pub fn for_severity(&self, severity: AlertSeverity) -> &str {
        match severity {
            AlertSeverity::Critical => &self.critical,
            AlertSeverity::Warning => &self.warning,
            AlertSeverity::Info => &self.info,
        }
    }
    pub fn normalized(mut self) -> Result<Self> {
        for value in [
            &mut self.critical,
            &mut self.warning,
            &mut self.info,
            &mut self.project_test,
        ] {
            *value = value.trim().to_string();
            if value.is_empty() {
                return Err(Error::validation("alert names must not be empty"));
            }
        }
        Ok(self)
    }
}

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
                ("service_name".into(), "{{ project_name }}".into()),
                ("issue_title".into(), "{{ title }}".into()),
                ("summary".into(), "{{ project_name }}: {{ title }}".into()),
                (
                    "description".into(),
                    "Error: {{ title }}\nLocation: {{ culprit }}\nEnvironment: {{ environment }}\nRelease: {{ release }}\nEvents: {{ event_count }}\nLast seen: {{ last_seen }}\n\nSoika: {{ issue_url }}".into(),
                ),
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

/// Only scalar substitution and a date format filter; no recursive expansion. JSON encoding
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
        let expression = tail[..end].trim();
        let (name, filter) = expression
            .split_once('|')
            .map_or((expression, None), |(name, filter)| {
                (name.trim(), Some(filter.trim()))
            });
        if !VARIABLES.contains(&name) {
            return Err(Error::validation(format!(
                "unknown notification variable: {name}"
            )));
        }
        let value = variables
            .get(name)
            .ok_or_else(|| Error::validation("missing notification variable"))?;
        if matches!(name, "first_seen" | "last_seen") {
            let format = match filter {
                None => "%-d %B %Y, %H:%M",
                Some(filter) => date_format(filter)?,
            };
            let items: Vec<_> = chrono::format::StrftimeItems::new(format).collect();
            if items.contains(&chrono::format::Item::Error) {
                return Err(Error::validation("invalid notification date format"));
            }
            let date = chrono::DateTime::parse_from_rfc3339(value)
                .map_err(|_| Error::validation("invalid notification timestamp"))?
                .with_timezone(&chrono::Local);
            out.push_str(
                &date
                    .format_localized_with_items(items.iter(), chrono::Locale::ru_RU)
                    .to_string(),
            );
        } else {
            if filter.is_some() {
                return Err(Error::validation(
                    "date filters require first_seen or last_seen",
                ));
            }
            out.push_str(value);
        }
        remaining = &tail[end + 2..];
    }
    if remaining.contains("}}") {
        return Err(Error::validation("invalid notification placeholder"));
    }
    out.push_str(remaining);
    Ok(out)
}

fn date_format(filter: &str) -> Result<&str> {
    let argument = filter
        .strip_prefix("date(")
        .and_then(|s| s.strip_suffix(')'))
        .map(str::trim)
        .ok_or_else(|| Error::validation("expected date(\"format\") filter"))?;
    for quote in ['\'', '"'] {
        if let Some(format) = argument
            .strip_prefix(quote)
            .and_then(|s| s.strip_suffix(quote))
            && !format.contains(quote)
        {
            return Ok(format);
        }
    }
    Err(Error::validation("date format must be a quoted string"))
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
    #[serde(default)]
    pub last_sent_at: Option<super::Timestamp>,
    pub issue_id: super::Id,
    pub endpoint: String,
    pub trigger: String,
    pub alert: AlertmanagerAlert,
}

/// Refresh cadence, owned by project management rather than channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertmanagerInterval {
    pub interval_seconds: Option<i64>,
    pub effective_interval_seconds: i64,
}

pub fn validate_interval(seconds: i64) -> Result<()> {
    if !(10..=86400).contains(&seconds) {
        return Err(Error::validation(
            "Alertmanager interval must be between 10 and 86400 seconds",
        ));
    }
    Ok(())
}

/// How an alert's severity is selected. Project None inherits the global mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityMode {
    #[default]
    Fixed,
    Event,
}
impl SeverityMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Event => "event",
        }
    }
}
impl std::str::FromStr for SeverityMode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "fixed" => Ok(Self::Fixed),
            "event" => Ok(Self::Event),
            _ => Err(Error::validation("severity mode must be fixed or event")),
        }
    }
}

/// Monotonic rank stored independently of the original SDK level. Zero is unknown.
pub fn event_severity_rank(level: Option<&str>) -> i32 {
    match level.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "fatal" | "error" => 3,
        "warning" => 2,
        "info" | "debug" => 1,
        _ => 0,
    }
}

/// Project alert classes. Event levels remain unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Critical,
    Warning,
    Info,
}
impl AlertSeverity {
    pub fn from_event_rank(rank: i32) -> Option<Self> {
        match rank {
            3 => Some(Self::Critical),
            2 => Some(Self::Warning),
            1 => Some(Self::Info),
            _ => None,
        }
    }

    pub fn alert_name(self) -> &'static str {
        match self {
            Self::Critical => "SoikaIssueCritical",
            Self::Warning => "SoikaIssueWarning",
            Self::Info => "SoikaIssueInfo",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}
impl std::str::FromStr for AlertSeverity {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "critical" => Ok(Self::Critical),
            "warning" => Ok(Self::Warning),
            "info" => Ok(Self::Info),
            _ => Err(Error::validation(
                "severity must be critical, warning or info",
            )),
        }
    }
}
