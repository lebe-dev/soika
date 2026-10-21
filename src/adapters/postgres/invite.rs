//! PostgreSQL [`InviteRepository`] adapter.

use super::{Db, now, ts};
use crate::domain::{Id, Invite, TeamRole, Timestamp};
use crate::error::Result;
use crate::ports::{InviteRepository, NewInvite};
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct PgInviteRepository {
    db: Db,
}

impl PgInviteRepository {
    pub fn new(db: Db) -> Self {
        PgInviteRepository { db }
    }
}

#[derive(FromRow)]
struct InviteRow {
    token: String,
    team_id: Id,
    role: String,
    email: Option<String>,
    created_by: Option<Id>,
    created_at: Timestamp,
    expires_at: Timestamp,
    accepted_at: Option<Timestamp>,
}

impl From<InviteRow> for Invite {
    fn from(row: InviteRow) -> Self {
        Invite {
            token: row.token,
            team_id: row.team_id,
            role: TeamRole::from_db(&row.role),
            email: row.email,
            created_by: row.created_by,
            created_at: row.created_at,
            expires_at: row.expires_at,
            accepted_at: row.accepted_at,
        }
    }
}

const SELECT_INVITE: &str = "SELECT token, team_id, role, email, created_by, created_at, \
    expires_at, accepted_at FROM invites";

#[async_trait]
impl InviteRepository for PgInviteRepository {
    async fn create(&self, new: NewInvite) -> Result<Invite> {
        let now = now();
        let expires_at = ts(new.expires_at);

        sqlx::query(
            "INSERT INTO invites (token, team_id, role, email, created_by, created_at, \
             expires_at, accepted_at) VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)",
        )
        .bind(&new.token)
        .bind(new.team_id)
        .bind(new.role.as_str())
        .bind(new.email.as_deref())
        .bind(new.created_by)
        .bind(now)
        .bind(expires_at)
        .execute(&self.db)
        .await?;

        Ok(Invite {
            token: new.token,
            team_id: new.team_id,
            role: new.role,
            email: new.email,
            created_by: new.created_by,
            created_at: now,
            expires_at,
            accepted_at: None,
        })
    }

    async fn find_by_token(&self, token: &str) -> Result<Option<Invite>> {
        let row = sqlx::query_as::<_, InviteRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_INVITE} WHERE token = $1"
        )))
        .bind(token)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(Invite::from))
    }

    async fn list_for_team(&self, team_id: Id) -> Result<Vec<Invite>> {
        let rows = sqlx::query_as::<_, InviteRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_INVITE} WHERE team_id = $1 ORDER BY created_at"
        )))
        .bind(team_id)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(Invite::from).collect())
    }

    async fn mark_accepted(&self, token: &str, accepted_at: Timestamp) -> Result<()> {
        sqlx::query("UPDATE invites SET accepted_at = $1 WHERE token = $2")
            .bind(ts(accepted_at))
            .bind(token)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn delete(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM invites WHERE token = $1")
            .bind(token)
            .execute(&self.db)
            .await?;
        Ok(())
    }
}
