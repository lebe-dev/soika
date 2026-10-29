//! SQLite [`TeamRepository`] adapter.

use super::{Db, bool_from_db, conflict_or_db, id_from_db, id_to_db, ts_from_db, ts_to_db};
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::domain::{
    AuthProvider, Id, InstanceRole, Team, TeamMember, TeamRole, Timestamp, User, UserStatus,
};
use crate::error::{Error, Result};
use crate::ports::TeamRepository;
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct SqliteTeamRepository {
    db: Db,
}

impl SqliteTeamRepository {
    pub fn new(db: Db) -> Self {
        SqliteTeamRepository { db }
    }
    /// Call only after acquiring the adapter's write/membership lock.
    async fn require_another_admin(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        team_id: Id,
        user_id: Id,
    ) -> Result<()> {
        let last_admin: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM team_members m WHERE m.team_id = ? AND m.user_id = ? \
             AND m.role = 'admin' AND NOT EXISTS (SELECT 1 FROM team_members other WHERE \
             other.team_id = m.team_id AND other.role = 'admin' AND other.user_id <> \
             m.user_id)",
        )
        .bind(id_to_db(team_id))
        .bind(id_to_db(user_id))
        .fetch_one(&mut **tx)
        .await?;
        if last_admin > 0 {
            return Err(Error::Conflict(
                "cannot remove or demote the last admin of the team".into(),
            ));
        }

        Ok(())
    }
}

#[derive(FromRow)]
struct TeamRow {
    id: String,
    name: String,
    created_at: String,
    updated_at: String,
    orphaned_at: Option<String>,
}

impl TeamRow {
    fn into_domain(self) -> Result<Team> {
        Ok(Team {
            orphaned_at: self.orphaned_at.as_deref().map(ts_from_db).transpose()?,
            id: id_from_db(&self.id)?,
            name: self.name,
            created_at: ts_from_db(&self.created_at)?,
            updated_at: ts_from_db(&self.updated_at)?,
        })
    }
}

/// Row used when joining `team_members` with `users` for [`TeamRepository::members`].
/// Carries both the user columns and the membership's `team_members.role`.
#[derive(FromRow)]
struct MemberUserRow {
    id: String,
    email: String,
    display_name: String,
    password_hash: String,
    instance_role: String,
    notifications_enabled: i64,
    locale: Option<String>,
    auth_provider: String,
    status: String,
    created_at: String,
    updated_at: String,
    role: String,
}

impl MemberUserRow {
    fn into_domain(self) -> Result<(User, TeamRole)> {
        let user = User {
            id: id_from_db(&self.id)?,
            email: self.email,
            display_name: self.display_name,
            password_hash: self.password_hash,
            instance_role: InstanceRole::from_db(&self.instance_role),
            notifications_enabled: bool_from_db(self.notifications_enabled),
            locale: self.locale.as_deref().map(str::parse).transpose()?,
            auth_provider: AuthProvider::from_db(&self.auth_provider),
            status: UserStatus::from_db(&self.status),
            created_at: ts_from_db(&self.created_at)?,
            updated_at: ts_from_db(&self.updated_at)?,
        };
        Ok((user, TeamRole::from_db(&self.role)))
    }
}

const SELECT_TEAM: &str = "SELECT id, name, created_at, updated_at, orphaned_at FROM teams";

#[async_trait]
impl TeamRepository for SqliteTeamRepository {
    async fn create(&self, name: String) -> Result<Team> {
        let id = Id::new_v4();
        let now: Timestamp = chrono::Utc::now();

        sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(id_to_db(id))
            .bind(&name)
            .bind(ts_to_db(now))
            .bind(ts_to_db(now))
            .execute(&self.db)
            .await
            .map_err(|e| conflict_or_db(e, format!("team name already exists: {name}")))?;

        Ok(Team {
            orphaned_at: None,
            id,
            name,
            created_at: now,
            updated_at: now,
        })
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Team>> {
        let row = sqlx::query_as::<_, TeamRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_TEAM} WHERE id = ?"
        )))
        .bind(id_to_db(id))
        .fetch_optional(&self.db)
        .await?;
        row.map(TeamRow::into_domain).transpose()
    }

    async fn list(&self) -> Result<Vec<Team>> {
        let rows = sqlx::query_as::<_, TeamRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_TEAM} ORDER BY name"
        )))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(TeamRow::into_domain).collect()
    }

    async fn rename(&self, id: Id, name: String) -> Result<Team> {
        let now: Timestamp = chrono::Utc::now();
        let result = sqlx::query("UPDATE teams SET name = ?, updated_at = ? WHERE id = ?")
            .bind(&name)
            .bind(ts_to_db(now))
            .bind(id_to_db(id))
            .execute(&self.db)
            .await
            .map_err(|e| conflict_or_db(e, format!("team name already exists: {name}")))?;

        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("team not found: {id}")));
        }

        // Re-read so the returned value carries the authoritative timestamps.
        self.find_by_id(id)
            .await?
            .ok_or_else(|| Error::not_found(format!("team not found: {id}")))
    }

    async fn delete(&self, id: Id, move_projects_to: Option<Id>) -> Result<()> {
        if move_projects_to == Some(id) {
            return Err(Error::validation(
                "target team must differ from deleted team",
            ));
        }
        super::write::retry_write(
            &crate::config::WritePolicy::default(),
            "delete team",
            || async {
                let mut tx = super::write::begin_write(&self.db).await?;
                if let Some(target) = move_projects_to {
                    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM teams WHERE id = ?")
                        .bind(id_to_db(target))
                        .fetch_one(&mut *tx)
                        .await?;
                    if exists == 0 {
                        return Err(Error::not_found("target team not found"));
                    }
                    sqlx::query(
                        "UPDATE projects SET team_id = ?, updated_at = ? WHERE team_id = ?",
                    )
                    .bind(id_to_db(target))
                    .bind(ts_to_db(chrono::Utc::now()))
                    .bind(id_to_db(id))
                    .execute(&mut *tx)
                    .await?;
                }
                let count: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE team_id = ?")
                        .bind(id_to_db(id))
                        .fetch_one(&mut *tx)
                        .await?;
                if count > 0 {
                    return Err(Error::Conflict(
                        "team still owns projects; move them to another team first".into(),
                    ));
                }
                sqlx::query("DELETE FROM teams WHERE id = ?")
                    .bind(id_to_db(id))
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                Ok(())
            },
        )
        .await
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Team>> {
        let rows = sqlx::query_as::<_, TeamRow>(
            "SELECT t.id, t.name, t.created_at, t.updated_at, t.orphaned_at FROM teams t \
             JOIN team_members m ON m.team_id = t.id \
             WHERE m.user_id = ? ORDER BY t.name",
        )
        .bind(id_to_db(user_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(TeamRow::into_domain).collect()
    }

    async fn is_member(&self, team_id: Id, user_id: Id) -> Result<bool> {
        let row =
            sqlx::query("SELECT 1 FROM team_members WHERE team_id = ? AND user_id = ? LIMIT 1")
                .bind(id_to_db(team_id))
                .bind(id_to_db(user_id))
                .fetch_optional(&self.db)
                .await?;
        Ok(row.is_some())
    }

    async fn add_member(&self, team_id: Id, user_id: Id, role: TeamRole) -> Result<TeamMember> {
        retry_write(&WritePolicy::default(), "team add_member", || async {
            let mut tx = super::write::begin_write(&self.db).await?;
            if role != TeamRole::Admin {
                self.require_another_admin(&mut tx, team_id, user_id)
                    .await?;
            }
            let result = async {
                let now: Timestamp = chrono::Utc::now();
                // Idempotent join: on a duplicate (team_id, user_id) update the role so a
                // re-add can also re-assign the team role.
                sqlx::query(
            "INSERT INTO team_members (team_id, user_id, role, created_at) VALUES (?, ?, ?, ?) \
             ON CONFLICT (team_id, user_id) DO UPDATE SET role = excluded.role",
        )
        .bind(id_to_db(team_id))
        .bind(id_to_db(user_id))
        .bind(role.as_str())
        .bind(ts_to_db(now))
        .execute(&mut *tx)
        .await?;

                Ok::<TeamMember, Error>(TeamMember {
                    team_id,
                    user_id,
                    role,
                    created_at: now,
                })
            }
            .await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }

    async fn set_member_role(
        &self,
        team_id: Id,
        user_id: Id,
        role: TeamRole,
    ) -> Result<TeamMember> {
        retry_write(&WritePolicy::default(), "team set_member_role", || async {
            let mut tx = super::write::begin_write(&self.db).await?;
            if role != TeamRole::Admin {
                self.require_another_admin(&mut tx, team_id, user_id)
                    .await?;
            }
            let result = async {
                let result = sqlx::query(
                    "UPDATE team_members SET role = ? WHERE team_id = ? AND user_id = ?",
                )
                .bind(role.as_str())
                .bind(id_to_db(team_id))
                .bind(id_to_db(user_id))
                .execute(&mut *tx)
                .await?;
                if result.rows_affected() == 0 {
                    return Err(Error::not_found(format!(
                        "team membership not found: team {team_id}, user {user_id}"
                    )));
                }

                // Re-read so the returned value carries the authoritative created_at.
                let row = sqlx::query_as::<_, (String,)>(
                    "SELECT created_at FROM team_members WHERE team_id = ? AND user_id = ?",
                )
                .bind(id_to_db(team_id))
                .bind(id_to_db(user_id))
                .fetch_one(&mut *tx)
                .await?;
                Ok::<TeamMember, Error>(TeamMember {
                    team_id,
                    user_id,
                    role,
                    created_at: ts_from_db(&row.0)?,
                })
            }
            .await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }

    async fn remove_member(&self, team_id: Id, user_id: Id) -> Result<()> {
        retry_write(&WritePolicy::default(), "team remove_member", || async {
            let mut tx = super::write::begin_write(&self.db).await?;
            self.require_another_admin(&mut tx, team_id, user_id)
                .await?;
            let result = async {
                sqlx::query("DELETE FROM team_members WHERE team_id = ? AND user_id = ?")
                    .bind(id_to_db(team_id))
                    .bind(id_to_db(user_id))
                    .execute(&mut *tx)
                    .await?;
                Ok::<(), Error>(())
            }
            .await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }

    async fn member_role(&self, team_id: Id, user_id: Id) -> Result<Option<TeamRole>> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT role FROM team_members WHERE team_id = ? AND user_id = ?",
        )
        .bind(id_to_db(team_id))
        .bind(id_to_db(user_id))
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(|(role,)| TeamRole::from_db(&role)))
    }

    async fn members(&self, team_id: Id) -> Result<Vec<(User, TeamRole)>> {
        let rows = sqlx::query_as::<_, MemberUserRow>(
            "SELECT u.id, u.email, u.display_name, u.password_hash, u.instance_role, \
             u.notifications_enabled, u.locale, u.auth_provider, u.status, u.created_at, u.updated_at, \
             m.role \
             FROM users u JOIN team_members m ON m.user_id = u.id \
             WHERE m.team_id = ? ORDER BY u.display_name",
        )
        .bind(id_to_db(team_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(MemberUserRow::into_domain).collect()
    }
}
