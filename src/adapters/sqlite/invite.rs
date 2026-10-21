//! SQLite [`InviteRepository`] adapter.

use super::{Db, id_from_db, id_to_db, ts_from_db, ts_to_db};
use crate::domain::{Id, Invite, TeamRole, Timestamp};
use crate::error::Result;
use crate::ports::{InviteRepository, NewInvite};
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct SqliteInviteRepository {
    db: Db,
}

impl SqliteInviteRepository {
    pub fn new(db: Db) -> Self {
        SqliteInviteRepository { db }
    }
}

#[derive(FromRow)]
struct InviteRow {
    token: String,
    team_id: String,
    role: String,
    email: Option<String>,
    created_by: Option<String>,
    created_at: String,
    expires_at: String,
    accepted_at: Option<String>,
}

impl InviteRow {
    fn into_domain(self) -> Result<Invite> {
        let created_by = match self.created_by {
            Some(s) => Some(id_from_db(&s)?),
            None => None,
        };
        let accepted_at = match self.accepted_at {
            Some(s) => Some(ts_from_db(&s)?),
            None => None,
        };
        Ok(Invite {
            token: self.token,
            team_id: id_from_db(&self.team_id)?,
            role: TeamRole::from_db(&self.role),
            email: self.email,
            created_by,
            created_at: ts_from_db(&self.created_at)?,
            expires_at: ts_from_db(&self.expires_at)?,
            accepted_at,
        })
    }
}

const SELECT_INVITE: &str = "SELECT token, team_id, role, email, created_by, created_at, \
    expires_at, accepted_at FROM invites";

#[async_trait]
impl InviteRepository for SqliteInviteRepository {
    async fn create(&self, new: NewInvite) -> Result<Invite> {
        let now: Timestamp = chrono::Utc::now();

        sqlx::query(
            "INSERT INTO invites (token, team_id, role, email, created_by, created_at, \
             expires_at, accepted_at) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind(&new.token)
        .bind(id_to_db(new.team_id))
        .bind(new.role.as_str())
        .bind(new.email.as_deref())
        .bind(new.created_by.map(id_to_db))
        .bind(ts_to_db(now))
        .bind(ts_to_db(new.expires_at))
        .execute(&self.db)
        .await?;

        Ok(Invite {
            token: new.token,
            team_id: new.team_id,
            role: new.role,
            email: new.email,
            created_by: new.created_by,
            created_at: now,
            expires_at: new.expires_at,
            accepted_at: None,
        })
    }

    async fn find_by_token(&self, token: &str) -> Result<Option<Invite>> {
        let row = sqlx::query_as::<_, InviteRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_INVITE} WHERE token = ?"
        )))
        .bind(token)
        .fetch_optional(&self.db)
        .await?;
        row.map(InviteRow::into_domain).transpose()
    }

    async fn list_for_team(&self, team_id: Id) -> Result<Vec<Invite>> {
        let rows = sqlx::query_as::<_, InviteRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_INVITE} WHERE team_id = ? ORDER BY created_at"
        )))
        .bind(id_to_db(team_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(InviteRow::into_domain).collect()
    }

    async fn mark_accepted(&self, token: &str, accepted_at: Timestamp) -> Result<()> {
        sqlx::query("UPDATE invites SET accepted_at = ? WHERE token = ?")
            .bind(ts_to_db(accepted_at))
            .bind(token)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn delete(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM invites WHERE token = ?")
            .bind(token)
            .execute(&self.db)
            .await?;
        Ok(())
    }
}
