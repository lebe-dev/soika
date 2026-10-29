//! PostgreSQL [`TeamRepository`] adapter.

use super::user::UserRow;
use super::{Db, conflict_or_db, now};
use crate::domain::{Id, Team, TeamMember, TeamRole, Timestamp, User};
use crate::error::{Error, Result};
use crate::ports::TeamRepository;
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct PgTeamRepository {
    db: Db,
}

impl PgTeamRepository {
    pub fn new(db: Db) -> Self {
        PgTeamRepository { db }
    }
}

#[derive(FromRow)]
struct TeamRow {
    id: Id,
    name: String,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl From<TeamRow> for Team {
    fn from(row: TeamRow) -> Self {
        Team {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Row used when joining `team_members` with `users` for [`TeamRepository::members`]:
/// the user columns plus the membership's `team_members.role`.
#[derive(FromRow)]
struct MemberUserRow {
    #[sqlx(flatten)]
    user: UserRow,
    role: String,
}

impl MemberUserRow {
    fn into_domain(self) -> Result<(User, TeamRole)> {
        Ok((self.user.into_domain()?, TeamRole::from_db(&self.role)))
    }
}

const SELECT_TEAM: &str = "SELECT id, name, created_at, updated_at FROM teams";

#[async_trait]
impl TeamRepository for PgTeamRepository {
    async fn create(&self, name: String) -> Result<Team> {
        let id = Id::new_v4();
        let now = now();

        sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES ($1, $2, $3, $3)")
            .bind(id)
            .bind(&name)
            .bind(now)
            .execute(&self.db)
            .await
            .map_err(|e| conflict_or_db(e, format!("team name already exists: {name}")))?;

        Ok(Team {
            id,
            name,
            created_at: now,
            updated_at: now,
        })
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Team>> {
        let row = sqlx::query_as::<_, TeamRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_TEAM} WHERE id = $1"
        )))
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(Team::from))
    }

    async fn list(&self) -> Result<Vec<Team>> {
        let rows = sqlx::query_as::<_, TeamRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_TEAM} ORDER BY name"
        )))
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(Team::from).collect())
    }

    async fn rename(&self, id: Id, name: String) -> Result<Team> {
        let row = sqlx::query_as::<_, TeamRow>(
            "UPDATE teams SET name = $1, updated_at = $2 WHERE id = $3 \
             RETURNING id, name, created_at, updated_at",
        )
        .bind(&name)
        .bind(now())
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| conflict_or_db(e, format!("team name already exists: {name}")))?;

        row.map(Team::from)
            .ok_or_else(|| Error::not_found(format!("team not found: {id}")))
    }

    async fn delete(&self, id: Id, move_projects_to: Option<Id>) -> Result<()> {
        if move_projects_to == Some(id) {
            return Err(Error::validation(
                "target team must differ from deleted team",
            ));
        }
        crate::adapters::retry::retry_write(
            &crate::config::WritePolicy::default(),
            "delete team",
            || async {
                let mut tx = self.db.begin().await?;
                // Serialize transfers with concurrent project creation and team deletion.
                sqlx::query("LOCK TABLE teams, projects IN SHARE ROW EXCLUSIVE MODE")
                    .execute(&mut *tx)
                    .await?;
                if let Some(target) = move_projects_to {
                    let exists: i64 =
                        sqlx::query_scalar("SELECT COUNT(*) FROM teams WHERE id = $1")
                            .bind(target)
                            .fetch_one(&mut *tx)
                            .await?;
                    if exists == 0 {
                        return Err(Error::not_found("target team not found"));
                    }
                    sqlx::query(
                        "UPDATE projects SET team_id = $1, updated_at = $2 WHERE team_id = $3",
                    )
                    .bind(target)
                    .bind(now())
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                }
                let count: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE team_id = $1")
                        .bind(id)
                        .fetch_one(&mut *tx)
                        .await?;
                if count > 0 {
                    return Err(Error::Conflict(
                        "team still owns projects; move them to another team first".into(),
                    ));
                }
                sqlx::query("DELETE FROM teams WHERE id = $1")
                    .bind(id)
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
            "SELECT t.id, t.name, t.created_at, t.updated_at FROM teams t \
             JOIN team_members m ON m.team_id = t.id \
             WHERE m.user_id = $1 ORDER BY t.name",
        )
        .bind(user_id)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(Team::from).collect())
    }

    async fn is_member(&self, team_id: Id, user_id: Id) -> Result<bool> {
        let row =
            sqlx::query("SELECT 1 FROM team_members WHERE team_id = $1 AND user_id = $2 LIMIT 1")
                .bind(team_id)
                .bind(user_id)
                .fetch_optional(&self.db)
                .await?;
        Ok(row.is_some())
    }

    async fn add_member(&self, team_id: Id, user_id: Id, role: TeamRole) -> Result<TeamMember> {
        // Idempotent join: on a duplicate (team_id, user_id) update the role so a
        // re-add can also re-assign the team role.
        let created_at: Timestamp = sqlx::query_scalar(
            "INSERT INTO team_members (team_id, user_id, role, created_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (team_id, user_id) DO UPDATE SET role = EXCLUDED.role \
             RETURNING created_at",
        )
        .bind(team_id)
        .bind(user_id)
        .bind(role.as_str())
        .bind(now())
        .fetch_one(&self.db)
        .await?;

        Ok(TeamMember {
            team_id,
            user_id,
            role,
            created_at,
        })
    }

    async fn set_member_role(
        &self,
        team_id: Id,
        user_id: Id,
        role: TeamRole,
    ) -> Result<TeamMember> {
        let created_at: Option<Timestamp> = sqlx::query_scalar(
            "UPDATE team_members SET role = $1 WHERE team_id = $2 AND user_id = $3 \
             RETURNING created_at",
        )
        .bind(role.as_str())
        .bind(team_id)
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;

        let Some(created_at) = created_at else {
            return Err(Error::not_found(format!(
                "team membership not found: team {team_id}, user {user_id}"
            )));
        };
        Ok(TeamMember {
            team_id,
            user_id,
            role,
            created_at,
        })
    }

    async fn remove_member(&self, team_id: Id, user_id: Id) -> Result<()> {
        sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team_id)
            .bind(user_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn member_role(&self, team_id: Id, user_id: Id) -> Result<Option<TeamRole>> {
        let role: Option<String> =
            sqlx::query_scalar("SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2")
                .bind(team_id)
                .bind(user_id)
                .fetch_optional(&self.db)
                .await?;
        Ok(role.map(|role| TeamRole::from_db(&role)))
    }

    async fn members(&self, team_id: Id) -> Result<Vec<(User, TeamRole)>> {
        let rows = sqlx::query_as::<_, MemberUserRow>(
            "SELECT u.id, u.email, u.display_name, u.password_hash, u.instance_role, \
             u.notifications_enabled, u.locale, u.auth_provider, u.status, u.created_at, \
             u.updated_at, m.role \
             FROM users u JOIN team_members m ON m.user_id = u.id \
             WHERE m.team_id = $1 ORDER BY u.display_name",
        )
        .bind(team_id)
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(MemberUserRow::into_domain).collect()
    }
}
