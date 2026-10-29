//! Database implementation of atomic declarative management.
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::{
    Error, Result,
    domain::{Id, config_file::*},
    ports::ConfigSync,
};
use async_trait::async_trait;
use sqlx::{Database, Decode, Encode, Executor, Row, Type};
use std::borrow::Cow;

/// The database-specific representation and locking rules for config sync.
pub trait ConfigDatabase: Database + Sized {
    type Id: DbValue<Self> + std::fmt::Display + PartialEq;
    type Time: DbValue<Self>;
    fn id(id: Id) -> Self::Id;
    fn domain_id(id: Self::Id) -> Result<Id>;
    fn now() -> Self::Time;
    fn sql(sql: &'static str) -> sqlx::AssertSqlSafe<Cow<'static, str>>;
    fn rows_affected(result: Self::QueryResult) -> u64;
    fn begin(
        db: &sqlx::Pool<Self>,
    ) -> impl Future<Output = Result<sqlx::Transaction<'_, Self>>> + Send;
}

pub trait DbValue<DB: Database>:
    Clone + Send + Sync + Unpin + Type<DB> + for<'q> Encode<'q, DB> + for<'r> Decode<'r, DB>
{
}
impl<DB: Database, T> DbValue<DB> for T where
    T: Clone + Send + Sync + Unpin + Type<DB> + for<'q> Encode<'q, DB> + for<'r> Decode<'r, DB>
{
}

enum ManagedEntity {
    Project,
    Team,
}

pub struct SqlConfigSync<DB: Database> {
    db: sqlx::Pool<DB>,
    write: WritePolicy,
}
impl<DB> SqlConfigSync<DB>
where
    DB: ConfigDatabase,
    DB::Arguments: sqlx::IntoArguments<DB>,
    String: DbValue<DB>,
    Option<String>: for<'q> Encode<'q, DB>,
    i64: DbValue<DB>,
    for<'e> &'e mut DB::Connection: Executor<'e, Database = DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
    pub fn new(db: sqlx::Pool<DB>, write: WritePolicy) -> Self {
        Self { db, write }
    }
    async fn apply_once(
        &self,
        desired: &ConfigFile,
        settings: &ManagementSettings,
        retention: i64,
    ) -> Result<SyncReport> {
        let mut transaction = DB::begin(&self.db).await?;
        let tx = &mut transaction;

        let existing_teams: Vec<String> =
            sqlx::query_scalar::<DB, _>(DB::sql("SELECT name FROM teams"))
                .fetch_all(&mut **tx)
                .await?;
        let existing_projects: Vec<(String, String)> = sqlx::query_as::<DB, _>(DB::sql(
            "SELECT p.slug, t.name FROM projects p JOIN teams t ON t.id = p.team_id",
        ))
        .fetch_all(&mut **tx)
        .await?;
        validate_plan(desired, settings, &existing_teams, &existing_projects)?;
        let now = DB::now();
        let mut report = SyncReport {
            loaded: settings.project_source == ManagementSource::Yaml
                || settings.team_source == ManagementSource::Yaml,
            synced_at: Some(chrono::Utc::now()),
            projects_in_file: desired.projects.as_ref().map_or(0, Vec::len),
            teams_in_file: desired.teams.as_ref().map_or(0, Vec::len),
            ..Default::default()
        };
        if settings.team_source == ManagementSource::Ui {
            sqlx::query::<DB>(DB::sql(
                "UPDATE teams SET orphaned_at = NULL WHERE orphaned_at IS NOT NULL",
            ))
            .execute(&mut **tx)
            .await?;
        }
        for team in desired.teams.iter().flatten() {
            self.sync_team(tx, team, &now, &mut report).await?;
        }
        if settings.project_source == ManagementSource::Ui {
            sqlx::query::<DB>(DB::sql(
                "UPDATE projects SET orphaned_at = NULL WHERE orphaned_at IS NOT NULL",
            ))
            .execute(&mut **tx)
            .await?;
        }
        self.prune_and_sync_projects(tx, desired, settings, retention, &now, &mut report)
            .await?;
        self.populate_report(tx, &mut report).await?;
        transaction.commit().await?;
        Ok(report)
    }
    async fn upsert_team(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        team: &TeamConfig,
        now: &DB::Time,
        report: &mut SyncReport,
    ) -> Result<DB::Id> {
        let found = sqlx::query::<DB>(DB::sql("SELECT id, orphaned_at FROM teams WHERE name = ?"))
            .bind(team.name.clone())
            .fetch_optional(&mut **tx)
            .await?;
        let team_id = if let Some(row) = found {
            let id: DB::Id = row.try_get("id")?;
            let orphaned: Option<DB::Time> = row.try_get("orphaned_at")?;
            let current_emails: Vec<(String, String)> = sqlx::query_as::<DB, _>(DB::sql(
                "SELECT email, role FROM team_member_grants WHERE team_id = ?",
            ))
            .bind(id.clone())
            .fetch_all(&mut **tx)
            .await?;
            let current_groups: Vec<(String, String)> = sqlx::query_as::<DB, _>(DB::sql(
                "SELECT group_path, role FROM team_group_grants WHERE team_id = ?",
            ))
            .bind(id.clone())
            .fetch_all(&mut **tx)
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
                sqlx::query::<DB>(DB::sql(
                    "UPDATE teams SET orphaned_at = NULL, updated_at = ? WHERE id = ?",
                ))
                .bind(now.clone())
                .bind(id.clone())
                .execute(&mut **tx)
                .await?;
            } else if changed {
                report.record("team", &team.name, SyncActionKind::Update);
                sqlx::query::<DB>(DB::sql("UPDATE teams SET updated_at = ? WHERE id = ?"))
                    .bind(now.clone())
                    .bind(id.clone())
                    .execute(&mut **tx)
                    .await?;
            } else {
                report.record("team", &team.name, SyncActionKind::Unchanged);
            }
            id
        } else {
            let id = DB::id(Id::new_v4());
            sqlx::query::<DB>(DB::sql(
                "INSERT INTO teams(id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
            ))
            .bind(id.clone())
            .bind(team.name.clone())
            .bind(now.clone())
            .bind(now.clone())
            .execute(&mut **tx)
            .await?;
            report.record("team", &team.name, SyncActionKind::Create);
            id
        };

        Ok(team_id)
    }
    async fn sync_team(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        team: &TeamConfig,
        now: &DB::Time,
        report: &mut SyncReport,
    ) -> Result<()> {
        let team_id = self.upsert_team(tx, team, now, report).await?;
        sqlx::query::<DB>(DB::sql("DELETE FROM team_member_grants WHERE team_id = ?"))
            .bind(team_id.clone())
            .execute(&mut **tx)
            .await?;
        sqlx::query::<DB>(DB::sql("DELETE FROM team_group_grants WHERE team_id = ?"))
            .bind(team_id.clone())
            .execute(&mut **tx)
            .await?;
        for grant in team.members.iter().flatten() {
            sqlx::query::<DB>(DB::sql(
                "INSERT INTO team_member_grants(team_id, email, role) VALUES (?, ?, ?)",
            ))
            .bind(team_id.clone())
            .bind(grant.email.to_lowercase())
            .bind(grant.role.as_str().to_string())
            .execute(&mut **tx)
            .await?;
        }
        for grant in team.sso_groups.iter().flatten() {
            sqlx::query::<DB>(DB::sql(
                "INSERT INTO team_group_grants(team_id, group_path, role) VALUES (?, ?, ?)",
            ))
            .bind(team_id.clone())
            .bind(grant.group.to_lowercase())
            .bind(grant.role.as_str().to_string())
            .execute(&mut **tx)
            .await?;
        }
        // Recompute only declared teams; orphan teams keep their frozen membership.
        let users = sqlx::query::<DB>(DB::sql("SELECT id, email FROM users"))
            .fetch_all(&mut **tx)
            .await?;
        for user in users {
            let user_id: DB::Id = user.try_get("id")?;
            let email: String = user.try_get("email")?;
            self.reconcile(tx, &team_id, &user_id, &email, now).await?;
        }
        let admin_count: i64 = sqlx::query_scalar::<DB, _>(DB::sql(
            "SELECT COUNT(*) FROM team_members WHERE team_id = ? AND role = 'admin'",
        ))
        .bind(team_id.clone())
        .fetch_one(&mut **tx)
        .await?;
        if admin_count == 0 {
            tracing::warn!(team_id = %team_id, "configured team has no administrator");
        }

        Ok(())
    }
    async fn sync_project(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        p: &ProjectConfig,
        retention: i64,
        now: &DB::Time,
        report: &mut SyncReport,
    ) -> Result<()> {
        let team_id: DB::Id =
            sqlx::query_scalar::<DB, _>(DB::sql("SELECT id FROM teams WHERE name = ?"))
                .bind(p.team.clone())
                .fetch_optional(&mut **tx)
                .await?
                .ok_or_else(|| {
                    Error::validation(format!("project {} references an unknown team", p.slug))
                })?;
        let existing = sqlx::query::<DB>(DB::sql("SELECT id, name, team_id, retention_events, retention_days, webhook_url, orphaned_at FROM \
            projects WHERE slug = ?"))
            .bind(p.slug.clone())
            .fetch_optional(&mut **tx).await?;
        let events = p.retention_events.unwrap_or(retention);
        let days = p.retention_days.unwrap_or(0);
        if let Some(row) = existing {
            let id: DB::Id = row.try_get("id")?;
            let orphaned: Option<DB::Time> = row.try_get("orphaned_at")?;
            let changed = row.try_get::<String, _>("name")? != p.name
                || row.try_get::<DB::Id, _>("team_id")? != team_id
                || row.try_get::<i64, _>("retention_events")? != events
                || row.try_get::<i64, _>("retention_days")? != days
                || row.try_get::<Option<String>, _>("webhook_url")? != p.webhook_url;
            let action = plan_present(true, orphaned.is_some(), changed);
            if action != SyncActionKind::Unchanged {
                sqlx::query::<DB>(DB::sql("UPDATE projects SET name = ?, team_id = ?, retention_events = ?, retention_days = ?, \
            webhook_url = ?, orphaned_at = NULL, updated_at = ? WHERE id = ?"))
                    .bind(p.name.clone())
                    .bind(team_id.clone())
                    .bind(events)
                    .bind(days)
                    .bind(p.webhook_url.clone())
                    .bind(now.clone())
                    .bind(id.clone())
                    .execute(&mut **tx).await?;
            }
            report.record("project", &p.slug, action);
        } else {
            let id = DB::id(Id::new_v4());
            let short_id = crate::adapters::short_id::generate_short_id();
            let dsn = Id::new_v4().simple().to_string();
            sqlx::query::<DB>(DB::sql("INSERT INTO projects(id, short_id, team_id, name, slug, dsn_public_key, retention_events, \
            retention_days, webhook_url, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"))
                .bind(id.clone())
                .bind(short_id.clone())
                .bind(team_id.clone())
                .bind(p.name.clone())
                .bind(p.slug.clone())
                .bind(dsn.clone())
                .bind(events)
                .bind(days)
                .bind(p.webhook_url.clone())
                .bind(now.clone())
                .bind(now.clone())
                .execute(&mut **tx).await?;
            report.record("project", &p.slug, SyncActionKind::Create);
        }

        Ok(())
    }
    async fn delete_project_events(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        id: &DB::Id,
    ) -> Result<()> {
        loop {
            let result = sqlx::query::<DB>(DB::sql("DELETE FROM events WHERE id IN (SELECT id FROM events WHERE project_id = ? LIMIT ?)"))
                .bind(id.clone())
                .bind(self.write.delete_batch)
                .execute(&mut **tx).await?;
            let n = DB::rows_affected(result);
            if n == 0 {
                break;
            }
        }

        Ok(())
    }
    async fn prune_project(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        row: DB::Row,
        slug: &str,
        policy: OrphanPolicy,
        now: &DB::Time,
        report: &mut SyncReport,
    ) -> Result<()> {
        let id: DB::Id = row.try_get("id")?;
        match policy {
            OrphanPolicy::Readonly => {
                self.mark_orphan(tx, row, ManagedEntity::Project, slug, now, report)
                    .await?
            }
            OrphanPolicy::Delete => {
                self.delete_project_events(tx, &id).await?;
                sqlx::query::<DB>(DB::sql("DELETE FROM projects WHERE id = ?"))
                    .bind(id.clone())
                    .execute(&mut **tx)
                    .await?;
                report.record("project", slug, SyncActionKind::Delete);
            }
        }
        Ok(())
    }
    async fn prune_team(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        row: DB::Row,
        name: &str,
        policy: OrphanPolicy,
        now: &DB::Time,
        report: &mut SyncReport,
    ) -> Result<()> {
        let id: DB::Id = row.try_get("id")?;
        match policy {
            OrphanPolicy::Readonly => {
                self.mark_orphan(tx, row, ManagedEntity::Team, name, now, report)
                    .await?
            }
            OrphanPolicy::Delete => {
                let slugs: Vec<String> = sqlx::query_scalar::<DB, _>(DB::sql(
                    "SELECT slug FROM projects WHERE team_id = ?",
                ))
                .bind(id.clone())
                .fetch_all(&mut **tx)
                .await?;
                if !slugs.is_empty() {
                    return Err(Error::validation(format!(
                        "team {name} still owns projects: {}",
                        slugs.join(", ")
                    )));
                }
                sqlx::query::<DB>(DB::sql("DELETE FROM teams WHERE id = ?"))
                    .bind(id.clone())
                    .execute(&mut **tx)
                    .await?;
                report.record("team", name, SyncActionKind::Delete);
            }
        }

        Ok(())
    }
    async fn mark_orphan(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        row: DB::Row,
        entity: ManagedEntity,
        key: &str,
        now: &DB::Time,
        report: &mut SyncReport,
    ) -> Result<()> {
        if row.try_get::<Option<DB::Time>, _>("orphaned_at")?.is_some() {
            return Ok(());
        }
        let id: DB::Id = row.try_get("id")?;
        let (kind, sql) = match entity {
            ManagedEntity::Project => (
                "project",
                "UPDATE projects SET orphaned_at = ? WHERE id = ?",
            ),
            ManagedEntity::Team => ("team", "UPDATE teams SET orphaned_at = ? WHERE id = ?"),
        };
        sqlx::query::<DB>(DB::sql(sql))
            .bind(now.clone())
            .bind(id)
            .execute(&mut **tx)
            .await?;
        report.record(kind, key, SyncActionKind::Orphan);
        Ok(())
    }
    async fn populate_report(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        report: &mut SyncReport,
    ) -> Result<()> {
        report.orphaned_projects = sqlx::query_scalar::<DB, _>(DB::sql(
            "SELECT COUNT(*) FROM projects WHERE orphaned_at IS NOT NULL",
        ))
        .fetch_one(&mut **tx)
        .await?;
        report.orphaned_teams = sqlx::query_scalar::<DB, _>(DB::sql(
            "SELECT COUNT(*) FROM teams WHERE orphaned_at IS NOT NULL",
        ))
        .fetch_one(&mut **tx)
        .await?;
        let local_rows = sqlx::query::<DB>(DB::sql(
            "SELECT g.email, g.role, u.id AS user_id FROM team_member_grants g JOIN users u ON \
            LOWER(u.email) = g.email WHERE u.auth_provider = 'local'",
        ))
        .fetch_all(&mut **tx)
        .await?;
        for row in local_rows {
            let user_id: DB::Id = row.try_get("user_id")?;
            report.local_accounts.push(GrantBinding {
                email: row.try_get("email")?,
                role: crate::domain::TeamRole::from_db(&row.try_get::<String, _>("role")?),
                user_id: Some(DB::domain_id(user_id)?),
                local_account: true,
            });
        }

        Ok(())
    }
    async fn prune_and_sync_projects(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        desired: &ConfigFile,
        settings: &ManagementSettings,
        retention: i64,
        now: &DB::Time,
        report: &mut SyncReport,
    ) -> Result<()> {
        if let Some(projects) = &desired.projects {
            for p in projects {
                self.sync_project(tx, p, retention, now, report).await?;
            }
            let rows = sqlx::query::<DB>(DB::sql("SELECT id, slug, orphaned_at FROM projects"))
                .fetch_all(&mut **tx)
                .await?;
            for row in rows {
                let slug: String = row.try_get("slug")?;
                if projects.iter().any(|p| p.slug == slug) {
                    continue;
                }
                self.prune_project(tx, row, &slug, settings.project_orphan_policy, now, report)
                    .await?;
            }
        }
        if let Some(teams) = &desired.teams {
            let rows = sqlx::query::<DB>(DB::sql("SELECT id, name, orphaned_at FROM teams"))
                .fetch_all(&mut **tx)
                .await?;
            for row in rows {
                let name: String = row.try_get("name")?;
                if teams.iter().any(|t| t.name == name) {
                    continue;
                }
                self.prune_team(tx, row, &name, settings.team_orphan_policy, now, report)
                    .await?;
            }
            report.pending_grants = sqlx::query_scalar::<DB, i64>(DB::sql("SELECT COUNT(*) FROM team_member_grants g WHERE NOT EXISTS (SELECT 1 FROM users u WHERE \
            LOWER(u.email) = g.email)"))
                .fetch_one(&mut **tx).await? as usize;
            report.local_grants = sqlx::query_scalar::<DB, i64>(DB::sql("SELECT COUNT(*) FROM team_member_grants g JOIN users u ON LOWER(u.email) = g.email WHERE \
            u.auth_provider = 'local'"))
                .fetch_one(&mut **tx).await? as usize;
        }

        Ok(())
    }
    async fn reconcile(
        &self,
        tx: &mut sqlx::Transaction<'_, DB>,
        team: &DB::Id,
        user: &DB::Id,
        email: &str,
        now: &DB::Time,
    ) -> Result<bool> {
        let roles: Vec<String> = sqlx::query_scalar::<DB, _>(DB::sql("SELECT role FROM team_member_grants WHERE team_id = ? AND email = ? UNION ALL SELECT g.role \
            FROM team_group_grants g JOIN user_sso_groups u ON g.group_path = u.group_path WHERE g.team_id \
            = ? AND u.user_id = ?"))
            .bind(team.clone())
            .bind(email.to_lowercase())
            .bind(team.clone())
            .bind(user.clone())
            .fetch_all(&mut **tx).await?;
        let role = crate::domain::config_file::reconcile_memberships(
            roles.iter().map(|r| crate::domain::TeamRole::from_db(r)),
        )
        .map(|r| r.as_str());
        if let Some(role) = role {
            sqlx::query::<DB>(DB::sql("INSERT INTO team_members(team_id, user_id, role, created_at) VALUES (?, ?, ?, ?) ON \
            CONFLICT(team_id, user_id) DO UPDATE SET role = excluded.role"))
                .bind(team.clone())
                .bind(user.clone())
                .bind(role.to_string())
                .bind(now.clone())
                .execute(&mut **tx).await?;
        } else {
            let result = sqlx::query::<DB>(DB::sql(
                "DELETE FROM team_members WHERE team_id = ? AND user_id = ?",
            ))
            .bind(team.clone())
            .bind(user.clone())
            .execute(&mut **tx)
            .await?;
            let removed = DB::rows_affected(result);
            if removed > 0 {
                tracing::warn!(user_id = %user, "configuration removed team membership");
            }
        }
        Ok(role.is_some())
    }
}
#[async_trait]
impl<DB> ConfigSync for SqlConfigSync<DB>
where
    DB: ConfigDatabase,
    DB::Arguments: sqlx::IntoArguments<DB>,
    String: DbValue<DB>,
    Option<String>: for<'q> Encode<'q, DB>,
    i64: DbValue<DB>,
    for<'e> &'e mut DB::Connection: Executor<'e, Database = DB>,
    for<'s> &'s str: sqlx::ColumnIndex<DB::Row>,
    usize: sqlx::ColumnIndex<DB::Row>,
{
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
        let rows = sqlx::query::<DB>(DB::sql("SELECT g.email, g.role, u.id AS user_id, u.auth_provider FROM team_member_grants g LEFT JOIN \
            users u ON LOWER(u.email) = g.email WHERE g.team_id = ?"))
            .bind(DB::id(team))
            .fetch_all(&self.db).await?;
        for row in rows {
            let user_id: Option<DB::Id> = row.try_get("user_id")?;
            let user_id = user_id.map(DB::domain_id).transpose()?;
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
        let rows = sqlx::query::<DB>(DB::sql(
            "SELECT group_path, role FROM team_group_grants WHERE team_id = ?",
        ))
        .bind(DB::id(team))
        .fetch_all(&self.db)
        .await?;
        for row in rows {
            result.groups.push(GroupGrant {
                group: row.try_get("group_path")?,
                role: crate::domain::TeamRole::from_db(&row.try_get::<String, _>("role")?),
            });
        }
        let rows = sqlx::query::<DB>(DB::sql("SELECT u.user_id, g.group_path FROM team_group_grants g JOIN user_sso_groups u ON u.group_path \
            = g.group_path WHERE g.team_id = ?"))
            .bind(DB::id(team))
            .fetch_all(&self.db).await?;
        for row in rows {
            let user_id: DB::Id = row.try_get("user_id")?;
            result.sources.push(MembershipSource {
                user_id: DB::domain_id(user_id)?,
                source: row.try_get("group_path")?,
            });
        }
        Ok(result)
    }
    async fn reconcile_user(&self, user: Id, groups: &[String]) -> Result<bool> {
        retry_write(&self.write, "reconcile SSO grants", || async {
            let mut transaction = DB::begin(&self.db).await?;
            let tx = &mut transaction;

            let now = DB::now();
            let user_id = DB::id(user);
            sqlx::query::<DB>(DB::sql("DELETE FROM user_sso_groups WHERE user_id = ?"))
                .bind(user_id.clone())
                .execute(&mut **tx)
                .await?;
            for group in groups
                .iter()
                .map(|g| g.to_lowercase())
                .collect::<std::collections::BTreeSet<_>>()
            {
                sqlx::query::<DB>(DB::sql(
                    "INSERT INTO user_sso_groups(user_id, group_path, seen_at) VALUES (?, ?, ?)",
                ))
                .bind(user_id.clone())
                .bind(group)
                .bind(now.clone())
                .execute(&mut **tx)
                .await?;
            }
            let email: String =
                sqlx::query_scalar::<DB, _>(DB::sql("SELECT email FROM users WHERE id = ?"))
                    .bind(user_id.clone())
                    .fetch_one(&mut **tx)
                    .await?;
            let teams: Vec<DB::Id> = sqlx::query_scalar::<DB, _>(DB::sql(
                "SELECT id FROM teams WHERE orphaned_at IS NULL",
            ))
            .fetch_all(&mut **tx)
            .await?;
            let mut granted = false;
            for team in teams {
                granted |= self.reconcile(tx, &team, &user_id, &email, &now).await?;
            }
            transaction.commit().await?;
            Ok(granted)
        })
        .await
    }
}
