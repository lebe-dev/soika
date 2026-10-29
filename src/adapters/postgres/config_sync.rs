//! Database implementation of atomic declarative management.
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::{
    Error, Result,
    domain::{Id, config_file::*},
    ports::ConfigSync,
};
use async_trait::async_trait;
use sqlx::Row;
pub struct PgConfigSync {
    db: super::Db,
    write: WritePolicy,
}
impl PgConfigSync {
    pub fn new(db: super::Db, write: WritePolicy) -> Self {
        Self { db, write }
    }
}
impl PgConfigSync {
    async fn apply_once(
        &self,
        desired: &ConfigFile,
        settings: &ManagementSettings,
        retention: i64,
    ) -> Result<SyncReport> {
        let mut tx = self.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(9182609)")
            .execute(&mut *tx)
            .await?;

        let existing_teams: Vec<String> = sqlx::query_scalar("SELECT name FROM teams")
            .fetch_all(&mut *tx)
            .await?;
        let existing_projects: Vec<(String, String)> = sqlx::query_as(
            "SELECT p.slug, t.name FROM projects p JOIN teams t ON t.id = p.team_id",
        )
        .fetch_all(&mut *tx)
        .await?;
        validate_plan(desired, settings, &existing_teams, &existing_projects)?;
        let now = chrono::Utc::now();
        let mut report = SyncReport {
            loaded: settings.project_source == ManagementSource::Yaml
                || settings.team_source == ManagementSource::Yaml,
            synced_at: Some(chrono::Utc::now()),
            projects_in_file: desired.projects.as_ref().map_or(0, Vec::len),
            teams_in_file: desired.teams.as_ref().map_or(0, Vec::len),
            ..Default::default()
        };
        if settings.team_source == ManagementSource::Ui {
            sqlx::query("UPDATE teams SET orphaned_at = NULL WHERE orphaned_at IS NOT NULL")
                .execute(&mut *tx)
                .await?;
        }
        if let Some(teams) = &desired.teams {
            for team in teams {
                let found = sqlx::query("SELECT id, orphaned_at FROM teams WHERE name = $1")
                    .bind(&team.name)
                    .fetch_optional(&mut *tx)
                    .await?;
                let team_id = if let Some(row) = found {
                    let id: Id = row.try_get("id")?;
                    let orphaned: Option<chrono::DateTime<chrono::Utc>> =
                        row.try_get("orphaned_at")?;
                    let current_emails: Vec<(String, String)> = sqlx::query_as(
                        "SELECT email, role FROM team_member_grants WHERE team_id = $1",
                    )
                    .bind(id)
                    .fetch_all(&mut *tx)
                    .await?;
                    let current_groups: Vec<(String, String)> = sqlx::query_as(
                        "SELECT group_path, role FROM team_group_grants WHERE team_id = $1",
                    )
                    .bind(id)
                    .fetch_all(&mut *tx)
                    .await?;
                    let expected_emails: std::collections::BTreeSet<_> = team
                        .members
                        .iter()
                        .flatten()
                        .map(|g| (g.email.to_lowercase(), g.role.as_str().to_string()))
                        .collect();
                    let expected_groups: std::collections::BTreeSet<_> = team
                        .sso_groups
                        .iter()
                        .flatten()
                        .map(|g| (g.group.to_lowercase(), g.role.as_str().to_string()))
                        .collect();
                    let changed = current_emails
                        .into_iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        != expected_emails
                        || current_groups
                            .into_iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            != expected_groups;
                    if orphaned.is_some() {
                        report.record("team", &team.name, SyncActionKind::Restore);
                        sqlx::query(
                            "UPDATE teams SET orphaned_at = NULL, updated_at = $1 WHERE id = $2",
                        )
                        .bind(now)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                    } else if changed {
                        report.record("team", &team.name, SyncActionKind::Update);
                        sqlx::query("UPDATE teams SET updated_at = $1 WHERE id = $2")
                            .bind(now)
                            .bind(id)
                            .execute(&mut *tx)
                            .await?;
                    } else {
                        report.record("team", &team.name, SyncActionKind::Unchanged);
                    }
                    id
                } else {
                    let id = Id::new_v4();
                    sqlx::query("INSERT INTO teams(id, name, created_at, updated_at) VALUES ($1, $2, $3, $4)").bind(id).bind(&team.name).bind(now).bind(now).execute(&mut *tx).await?;
                    report.record("team", &team.name, SyncActionKind::Create);
                    id
                };
                sqlx::query("DELETE FROM team_member_grants WHERE team_id = $1")
                    .bind(team_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM team_group_grants WHERE team_id = $1")
                    .bind(team_id)
                    .execute(&mut *tx)
                    .await?;
                for grant in team.members.iter().flatten() {
                    sqlx::query(
                        "INSERT INTO team_member_grants(team_id, email, role) VALUES ($1, $2, $3)",
                    )
                    .bind(team_id)
                    .bind(grant.email.to_lowercase())
                    .bind(grant.role.as_str())
                    .execute(&mut *tx)
                    .await?;
                }
                for grant in team.sso_groups.iter().flatten() {
                    sqlx::query("INSERT INTO team_group_grants(team_id, group_path, role) VALUES ($1, $2, $3)").bind(team_id).bind(grant.group.to_lowercase()).bind(grant.role.as_str()).execute(&mut *tx).await?;
                }
                // Recompute only declared teams; orphan teams keep their frozen membership.
                let users = sqlx::query("SELECT id, email FROM users")
                    .fetch_all(&mut *tx)
                    .await?;
                for user in users {
                    let user_id: Id = user.try_get("id")?;
                    let email: String = user.try_get("email")?;
                    reconcile(&mut tx, &team_id, &user_id, &email, &now).await?;
                }
                let admin_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM team_members WHERE team_id = $1 AND role = 'admin'",
                )
                .bind(team_id)
                .fetch_one(&mut *tx)
                .await?;
                if admin_count == 0 {
                    tracing::warn!(team_id = %team_id, "configured team has no administrator");
                }
            }
        }
        if settings.project_source == ManagementSource::Ui {
            sqlx::query("UPDATE projects SET orphaned_at = NULL WHERE orphaned_at IS NOT NULL")
                .execute(&mut *tx)
                .await?;
        }
        if let Some(projects) = &desired.projects {
            for p in projects {
                if let Some(teams) = &desired.teams
                    && !teams.iter().any(|t| t.name == p.team)
                {
                    return Err(Error::validation(format!(
                        "project {} refers to a team absent from configuration",
                        p.slug
                    )));
                }
                let team_id: Id = sqlx::query_scalar("SELECT id FROM teams WHERE name = $1")
                    .bind(&p.team)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or_else(|| {
                        Error::validation(format!("project {} references an unknown team", p.slug))
                    })?;
                let existing = sqlx::query("SELECT id, name, team_id, retention_events, retention_days, webhook_url, orphaned_at FROM projects WHERE slug = $1").bind(&p.slug).fetch_optional(&mut *tx).await?;
                let events = p.retention_events.unwrap_or(retention);
                let days = p.retention_days.unwrap_or(0);
                if let Some(row) = existing {
                    let id: Id = row.try_get("id")?;
                    let orphaned: Option<chrono::DateTime<chrono::Utc>> =
                        row.try_get("orphaned_at")?;
                    let changed = row.try_get::<String, _>("name")? != p.name
                        || row.try_get::<Id, _>("team_id")? != team_id
                        || row.try_get::<i64, _>("retention_events")? != events
                        || row.try_get::<i64, _>("retention_days")? != days
                        || row.try_get::<Option<String>, _>("webhook_url")? != p.webhook_url;
                    let action = plan_present(true, orphaned.is_some(), changed);
                    if action != SyncActionKind::Unchanged {
                        sqlx::query("UPDATE projects SET name = $1, team_id = $2, retention_events = $3, retention_days = $4, webhook_url = $5, orphaned_at = NULL, updated_at = $6 WHERE id = $7").bind(&p.name).bind(team_id).bind(events).bind(days).bind(&p.webhook_url).bind(now).bind(id).execute(&mut *tx).await?;
                    }
                    report.record("project", &p.slug, action);
                } else {
                    let id = Id::new_v4();
                    let short_id = crate::adapters::short_id::generate_short_id();
                    let dsn = Id::new_v4().simple().to_string();
                    sqlx::query("INSERT INTO projects(id, short_id, team_id, name, slug, dsn_public_key, retention_events, retention_days, webhook_url, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)").bind(id).bind(&short_id).bind(team_id).bind(&p.name).bind(&p.slug).bind(&dsn).bind(events).bind(days).bind(&p.webhook_url).bind(now).bind(now).execute(&mut *tx).await?;
                    report.record("project", &p.slug, SyncActionKind::Create);
                }
            }
            let rows = sqlx::query("SELECT id, slug, orphaned_at FROM projects")
                .fetch_all(&mut *tx)
                .await?;
            for row in rows {
                let slug: String = row.try_get("slug")?;
                if projects.iter().any(|p| p.slug == slug) {
                    continue;
                }
                let id: Id = row.try_get("id")?;
                match settings.project_orphan_policy {
                    OrphanPolicy::Readonly => {
                        if row
                            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("orphaned_at")?
                            .is_none()
                        {
                            sqlx::query("UPDATE projects SET orphaned_at = $1 WHERE id = $2")
                                .bind(now)
                                .bind(id)
                                .execute(&mut *tx)
                                .await?;
                            report.record("project", &slug, SyncActionKind::Orphan);
                        }
                    }
                    OrphanPolicy::Delete => {
                        loop {
                            let n = sqlx::query("DELETE FROM events WHERE id IN (SELECT id FROM events WHERE project_id = $1 LIMIT $2)").bind(id).bind(self.write.delete_batch).execute(&mut *tx).await?.rows_affected();
                            if n == 0 {
                                break;
                            }
                        }
                        sqlx::query("DELETE FROM projects WHERE id = $1")
                            .bind(id)
                            .execute(&mut *tx)
                            .await?;
                        report.record("project", &slug, SyncActionKind::Delete);
                    }
                }
            }
        }
        if let Some(teams) = &desired.teams {
            let rows = sqlx::query("SELECT id, name, orphaned_at FROM teams")
                .fetch_all(&mut *tx)
                .await?;
            for row in rows {
                let name: String = row.try_get("name")?;
                if teams.iter().any(|t| t.name == name) {
                    continue;
                }
                let id: Id = row.try_get("id")?;
                match settings.team_orphan_policy {
                    OrphanPolicy::Readonly => {
                        if row
                            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("orphaned_at")?
                            .is_none()
                        {
                            sqlx::query("UPDATE teams SET orphaned_at = $1 WHERE id = $2")
                                .bind(now)
                                .bind(id)
                                .execute(&mut *tx)
                                .await?;
                            report.record("team", &name, SyncActionKind::Orphan);
                        }
                    }
                    OrphanPolicy::Delete => {
                        let slugs: Vec<String> =
                            sqlx::query_scalar("SELECT slug FROM projects WHERE team_id = $1")
                                .bind(id)
                                .fetch_all(&mut *tx)
                                .await?;
                        if !slugs.is_empty() {
                            return Err(Error::validation(format!(
                                "team {name} still owns projects: {}",
                                slugs.join(", ")
                            )));
                        }
                        sqlx::query("DELETE FROM teams WHERE id = $1")
                            .bind(id)
                            .execute(&mut *tx)
                            .await?;
                        report.record("team", &name, SyncActionKind::Delete);
                    }
                }
            }
            report.pending_grants = sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM team_member_grants g WHERE NOT EXISTS (SELECT 1 FROM users u WHERE LOWER(u.email) = g.email)").fetch_one(&mut *tx).await? as usize;
            report.local_grants = sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM team_member_grants g JOIN users u ON LOWER(u.email) = g.email WHERE u.auth_provider = 'local'").fetch_one(&mut *tx).await? as usize;
        }
        report.orphaned_projects =
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE orphaned_at IS NOT NULL")
                .fetch_one(&mut *tx)
                .await?;
        report.orphaned_teams =
            sqlx::query_scalar("SELECT COUNT(*) FROM teams WHERE orphaned_at IS NOT NULL")
                .fetch_one(&mut *tx)
                .await?;
        let local_rows = sqlx::query("SELECT g.email, g.role, u.id AS user_id FROM team_member_grants g JOIN users u ON LOWER(u.email) = g.email WHERE u.auth_provider = 'local'").fetch_all(&mut *tx).await?;
        for row in local_rows {
            let user_id: Id = row.try_get("user_id")?;
            report.local_accounts.push(GrantBinding {
                email: row.try_get("email")?,
                role: crate::domain::TeamRole::from_db(&row.try_get::<String, _>("role")?),
                user_id: Some(user_id),
                local_account: true,
            });
        }
        tx.commit().await?;
        Ok(report)
    }
}
#[async_trait]
impl ConfigSync for PgConfigSync {
    async fn apply(
        &self,
        desired: &ConfigFile,
        settings: &ManagementSettings,
        retention: i64,
    ) -> Result<SyncReport> {
        retry_write(&self.write, "synchronize configuration", || {
            self.apply_once(desired, settings, retention)
        })
        .await
    }
    async fn inspect_team(&self, team: Id) -> Result<TeamGrantView> {
        let mut result = TeamGrantView::default();
        let rows = sqlx::query("SELECT g.email, g.role, u.id AS user_id, u.auth_provider FROM team_member_grants g LEFT JOIN users u ON LOWER(u.email) = g.email WHERE g.team_id = $1").bind(team).fetch_all(&self.db).await?;
        for row in rows {
            let user_id: Option<Id> = row.try_get("user_id")?;
            result.members.push(GrantBinding {
                email: row.try_get("email")?,
                role: crate::domain::TeamRole::from_db(&row.try_get::<String, _>("role")?),
                user_id,
                local_account: row
                    .try_get::<Option<String>, _>("auth_provider")?
                    .as_deref()
                    == Some("local"),
            });
            if let Some(user_id) = user_id {
                result.sources.push(MembershipSource {
                    user_id,
                    source: "email".into(),
                });
            }
        }
        let rows = sqlx::query("SELECT group_path, role FROM team_group_grants WHERE team_id = $1")
            .bind(team)
            .fetch_all(&self.db)
            .await?;
        for row in rows {
            result.groups.push(GroupGrant {
                group: row.try_get("group_path")?,
                role: crate::domain::TeamRole::from_db(&row.try_get::<String, _>("role")?),
            });
        }
        let rows = sqlx::query("SELECT u.user_id, g.group_path FROM team_group_grants g JOIN user_sso_groups u ON u.group_path = g.group_path WHERE g.team_id = $1").bind(team).fetch_all(&self.db).await?;
        for row in rows {
            let user_id: Id = row.try_get("user_id")?;
            result.sources.push(MembershipSource {
                user_id,
                source: row.try_get("group_path")?,
            });
        }
        Ok(result)
    }
    async fn reconcile_user(&self, user: Id, groups: &[String]) -> Result<bool> {
        retry_write(&self.write, "reconcile SSO grants", || async {
            let mut tx = self.db.begin().await?;
            sqlx::query("SELECT pg_advisory_xact_lock(9182609)")
                .execute(&mut *tx)
                .await?;

            let now = chrono::Utc::now();
            let user_id = user;
            sqlx::query("DELETE FROM user_sso_groups WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            for group in groups
                .iter()
                .map(|g| g.to_lowercase())
                .collect::<std::collections::BTreeSet<_>>()
            {
                sqlx::query(
                    "INSERT INTO user_sso_groups(user_id, group_path, seen_at) VALUES ($1, $2, $3)",
                )
                .bind(user_id)
                .bind(group)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
            let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await?;
            let teams: Vec<Id> =
                sqlx::query_scalar("SELECT id FROM teams WHERE orphaned_at IS NULL")
                    .fetch_all(&mut *tx)
                    .await?;
            let mut granted = false;
            for team in teams {
                granted |= reconcile(&mut tx, &team, &user_id, &email, &now).await?;
            }
            tx.commit().await?;
            Ok(granted)
        })
        .await
    }
}
async fn reconcile(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    team: &Id,
    user: &Id,
    email: &str,
    now: &chrono::DateTime<chrono::Utc>,
) -> Result<bool> {
    let roles: Vec<String> = sqlx::query_scalar("SELECT role FROM team_member_grants WHERE team_id = $1 AND email = $2 UNION ALL SELECT g.role FROM team_group_grants g JOIN user_sso_groups u ON g.group_path = u.group_path WHERE g.team_id = $3 AND u.user_id = $4").bind(team).bind(email.to_lowercase()).bind(team).bind(user).fetch_all(&mut **tx).await?;
    let role = crate::domain::config_file::reconcile_memberships(
        roles.iter().map(|r| crate::domain::TeamRole::from_db(r)),
    )
    .map(|r| r.as_str());
    if let Some(role) = role {
        sqlx::query("INSERT INTO team_members(team_id, user_id, role, created_at) VALUES ($1, $2, $3, $4) ON CONFLICT(team_id, user_id) DO UPDATE SET role = excluded.role").bind(team).bind(user).bind(role).bind(now).execute(&mut **tx).await?;
    } else {
        let removed = sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team)
            .bind(user)
            .execute(&mut **tx)
            .await?
            .rows_affected();
        if removed > 0 {
            tracing::warn!(user_id = %user, "configuration removed team membership");
        }
    }
    Ok(role.is_some())
}
