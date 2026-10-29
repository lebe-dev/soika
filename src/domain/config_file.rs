//! Declarative management types and validation. No file or environment IO.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManagementSource {
    #[default]
    Ui,
    Yaml,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrphanPolicy {
    #[default]
    Readonly,
    Delete,
}
#[derive(Debug, Clone, Serialize)]
pub struct ManagementSettings {
    pub path: String,
    pub project_source: ManagementSource,
    pub team_source: ManagementSource,
    pub project_orphan_policy: OrphanPolicy,
    pub team_orphan_policy: OrphanPolicy,
}
impl Default for ManagementSettings {
    fn default() -> Self {
        Self {
            path: "/etc/soika/config.yaml".into(),
            project_source: ManagementSource::Ui,
            team_source: ManagementSource::Ui,
            project_orphan_policy: OrphanPolicy::Readonly,
            team_orphan_policy: OrphanPolicy::Readonly,
        }
    }
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub version: u32,
    pub projects: Option<Vec<ProjectConfig>>,
    pub teams: Option<Vec<TeamConfig>>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub slug: String,
    pub name: String,
    pub team: String,
    pub retention_events: Option<i64>,
    pub retention_days: Option<i64>,
    pub webhook_url: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamConfig {
    pub name: String,
    pub members: Option<Vec<EmailGrant>>,
    pub sso_groups: Option<Vec<GroupGrant>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmailGrant {
    pub email: String,
    pub role: super::TeamRole,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GroupGrant {
    pub group: String,
    pub role: super::TeamRole,
}

pub fn parse(text: &str, settings: &ManagementSettings) -> Result<ConfigFile> {
    let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).map_err(|_| {
        Error::validation("invalid configuration YAML (duplicate key or invalid value)")
    })?;
    discard_ui_sections(&mut value, settings);
    let mut file: ConfigFile = serde_yaml_ng::from_value(value).map_err(|_| {
        Error::validation("invalid configuration YAML (unknown field or invalid value)")
    })?;
    if file.version != 1 {
        return Err(Error::validation("configuration version must be 1"));
    }
    if settings.project_source == ManagementSource::Ui {
        file.projects = None;
    } else if file.projects.is_none() {
        return Err(Error::validation("projects section is required"));
    }
    if settings.team_source == ManagementSource::Ui {
        file.teams = None;
    } else if file.teams.is_none() {
        return Err(Error::validation("teams section is required"));
    }
    let mut slugs = BTreeSet::new();
    for p in file.projects.iter().flatten() {
        if p.slug.is_empty() || super::slugify(&p.slug) != p.slug || !slugs.insert(&p.slug) {
            return Err(Error::validation("invalid or duplicate project slug"));
        }
        if p.name.trim().is_empty() || p.team.trim().is_empty() {
            return Err(Error::validation("project name and team are required"));
        }
        if p.retention_events.is_some_and(|v| v < 1) || p.retention_days.is_some_and(|v| v < 0) {
            return Err(Error::validation("invalid project retention"));
        }
        if p.webhook_url
            .as_ref()
            .is_some_and(|v| !super::is_valid_webhook_url(v))
        {
            return Err(Error::validation("invalid webhook URL"));
        }
    }
    let mut teams = BTreeSet::new();
    for t in file.teams.iter().flatten() {
        if t.name.trim().is_empty() || !teams.insert(&t.name) {
            return Err(Error::validation("empty or duplicate team name"));
        }
        if t.members.is_none() && t.sso_groups.is_none() {
            return Err(Error::validation(
                "team requires members or sso_groups (use members: [] for an empty team)",
            ));
        }
        let mut emails = BTreeSet::new();
        for m in t.members.iter().flatten() {
            if !m.email.contains('@')
                || m.email.chars().any(char::is_whitespace)
                || !emails.insert(m.email.to_lowercase())
            {
                return Err(Error::validation("invalid or duplicate member email"));
            }
        }
        let mut groups = BTreeSet::new();
        for g in t.sso_groups.iter().flatten() {
            if g.group.trim().is_empty() || !groups.insert(g.group.clone()) {
                return Err(Error::validation("empty or duplicate SSO group"));
            }
        }
    }
    Ok(file)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub actions: Vec<SyncAction>,
    pub loaded: bool,
    pub default_file: bool,
    pub synced_at: Option<super::Timestamp>,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub restored: usize,
    pub orphaned: usize,
    pub deleted: usize,
    pub projects_in_file: usize,
    pub teams_in_file: usize,
    pub pending_grants: usize,
    pub local_grants: usize,
    pub local_accounts: Vec<GrantBinding>,
    pub orphaned_projects: i64,
    pub orphaned_teams: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TeamGrantView {
    pub members: Vec<GrantBinding>,
    pub groups: Vec<GroupGrant>,
    pub sources: Vec<MembershipSource>,
}
#[derive(Debug, Clone, Serialize)]
pub struct GrantBinding {
    pub email: String,
    pub role: super::TeamRole,
    pub user_id: Option<super::Id>,
    pub local_account: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct MembershipSource {
    pub user_id: super::Id,
    pub source: String,
}
/// Combine all membership sources, preferring Admin over Contributor.
pub fn reconcile_memberships(
    roles: impl IntoIterator<Item = super::TeamRole>,
) -> Option<super::TeamRole> {
    let mut result = None;
    for role in roles {
        if role == super::TeamRole::Admin {
            return Some(role);
        }
        result = Some(role);
    }
    result
}

/// Validate cross-entity references before any writes. Return all missing teams.
pub fn validate_references(file: &ConfigFile, existing_teams: &[String]) -> Result<()> {
    let names: BTreeSet<&str> = match &file.teams {
        Some(teams) => teams.iter().map(|team| team.name.as_str()).collect(),
        None => existing_teams.iter().map(String::as_str).collect(),
    };
    let errors: Vec<String> = file
        .projects
        .iter()
        .flatten()
        .filter(|project| !names.contains(project.team.as_str()))
        .map(|project| {
            format!(
                "project {} references unknown team {}",
                project.slug, project.team
            )
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::validation(errors.join("; ")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncActionKind {
    Create,
    Update,
    Unchanged,
    Restore,
    Orphan,
    Delete,
}
#[derive(Debug, Clone, Serialize)]
pub struct SyncAction {
    pub entity: String,
    pub key: String,
    pub action: SyncActionKind,
}
/// Pure planner for a declared entity; the adapter supplies its locked snapshot.
pub fn plan_present(exists: bool, orphaned: bool, changed: bool) -> SyncActionKind {
    if !exists {
        return SyncActionKind::Create;
    }
    if orphaned {
        return SyncActionKind::Restore;
    }
    if changed {
        return SyncActionKind::Update;
    }
    SyncActionKind::Unchanged
}
impl SyncReport {
    pub fn record(&mut self, entity: &str, key: &str, action: SyncActionKind) {
        match action {
            SyncActionKind::Create => self.created += 1,
            SyncActionKind::Update => self.updated += 1,
            SyncActionKind::Unchanged => self.unchanged += 1,
            SyncActionKind::Restore => self.restored += 1,
            SyncActionKind::Orphan => self.orphaned += 1,
            SyncActionKind::Delete => self.deleted += 1,
        }
        self.actions.push(SyncAction {
            entity: entity.into(),
            key: key.into(),
            action,
        });
    }
}

/// Ignore sections owned by the UI before interpreting or expanding their values.
pub fn discard_ui_sections(value: &mut serde_yaml_ng::Value, settings: &ManagementSettings) {
    if let serde_yaml_ng::Value::Mapping(map) = value {
        if settings.project_source == ManagementSource::Ui {
            map.remove(serde_yaml_ng::Value::String("projects".into()));
        }
        if settings.team_source == ManagementSource::Ui {
            map.remove(serde_yaml_ng::Value::String("teams".into()));
        }
    }
}

/// Validate the final ownership graph, including planned transfers and removals.
pub fn validate_plan(
    file: &ConfigFile,
    settings: &ManagementSettings,
    teams: &[String],
    projects: &[(String, String)],
) -> Result<()> {
    let mut errors = Vec::new();
    if let Err(error) = validate_references(file, teams) {
        errors.push(error.to_string());
    }
    if let Some(desired_teams) = &file.teams
        && settings.team_orphan_policy == OrphanPolicy::Delete
    {
        for team in teams
            .iter()
            .filter(|name| !desired_teams.iter().any(|t| &t.name == *name))
        {
            let remaining: Vec<&str> = projects
                .iter()
                .filter_map(|(slug, current_team)| {
                    let final_team = match &file.projects {
                        Some(desired) => match desired.iter().find(|p| &p.slug == slug) {
                            Some(p) => &p.team,
                            None if settings.project_orphan_policy == OrphanPolicy::Delete => {
                                return None;
                            }
                            None => current_team,
                        },
                        None => current_team,
                    };
                    if final_team == team {
                        Some(slug.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if !remaining.is_empty() {
                errors.push(format!(
                    "team {team} still owns projects: {}",
                    remaining.join(", ")
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::validation(errors.join("; ")))
    }
}
