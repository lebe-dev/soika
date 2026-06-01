//! SQLite [`InviteRepository`] adapter.

use super::{id_from_db, id_to_db, ts_from_db, ts_to_db, Db};
use crate::domain::{Id, Invite, Role, Timestamp};
use crate::error::Result;
use crate::ports::{InviteRepository, NewInvite};
use async_trait::async_trait;
use sqlx::FromRow;
use std::str::FromStr;

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
    project_id: String,
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
            project_id: id_from_db(&self.project_id)?,
            role: Role::from_str(&self.role)?,
            email: self.email,
            created_by,
            created_at: ts_from_db(&self.created_at)?,
            expires_at: ts_from_db(&self.expires_at)?,
            accepted_at,
        })
    }
}

const SELECT_INVITE: &str = "SELECT token, project_id, role, email, created_by, created_at, \
    expires_at, accepted_at FROM invites";

#[async_trait]
impl InviteRepository for SqliteInviteRepository {
    async fn create(&self, new: NewInvite) -> Result<Invite> {
        let now: Timestamp = chrono::Utc::now();

        sqlx::query(
            "INSERT INTO invites (token, project_id, role, email, created_by, created_at, \
             expires_at, accepted_at) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind(&new.token)
        .bind(id_to_db(new.project_id))
        .bind(new.role.as_str())
        .bind(new.email.as_deref())
        .bind(new.created_by.map(id_to_db))
        .bind(ts_to_db(now))
        .bind(ts_to_db(new.expires_at))
        .execute(&self.db)
        .await?;

        Ok(Invite {
            token: new.token,
            project_id: new.project_id,
            role: new.role,
            email: new.email,
            created_by: new.created_by,
            created_at: now,
            expires_at: new.expires_at,
            accepted_at: None,
        })
    }

    async fn find_by_token(&self, token: &str) -> Result<Option<Invite>> {
        let row = sqlx::query_as::<_, InviteRow>(&format!("{SELECT_INVITE} WHERE token = ?"))
            .bind(token)
            .fetch_optional(&self.db)
            .await?;
        row.map(InviteRow::into_domain).transpose()
    }

    async fn list_for_project(&self, project_id: Id) -> Result<Vec<Invite>> {
        let rows = sqlx::query_as::<_, InviteRow>(&format!(
            "{SELECT_INVITE} WHERE project_id = ? ORDER BY created_at"
        ))
        .bind(id_to_db(project_id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::{insert_project, insert_team, insert_user, test_pool};
    use chrono::Duration;

    async fn fixtures(pool: &Db) -> (Id, Id) {
        let team_id = insert_team(pool, "T").await;
        let project_id = insert_project(pool, team_id, "p", "dsn").await;
        let user_id = insert_user(pool, "admin@example.com").await;
        (project_id, user_id)
    }

    fn new_invite(project_id: Id, created_by: Id, token: &str) -> NewInvite {
        NewInvite {
            token: token.to_string(),
            project_id,
            role: Role::Member,
            email: Some("invitee@example.com".to_string()),
            created_by: Some(created_by),
            expires_at: chrono::Utc::now() + Duration::days(7),
        }
    }

    #[tokio::test]
    async fn create_find_roundtrip() {
        let pool = test_pool().await;
        let (project_id, user_id) = fixtures(&pool).await;
        let repo = SqliteInviteRepository::new(pool.clone());

        let created = repo
            .create(new_invite(project_id, user_id, "tok"))
            .await
            .unwrap();
        assert_eq!(created.role, Role::Member);
        assert_eq!(created.email.as_deref(), Some("invitee@example.com"));
        assert_eq!(created.created_by, Some(user_id));
        assert!(created.accepted_at.is_none());

        let found = repo.find_by_token("tok").await.unwrap().unwrap();
        assert_eq!(found.token, "tok");
        assert_eq!(found.project_id, project_id);

        assert!(repo.find_by_token("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn create_without_email_or_creator() {
        let pool = test_pool().await;
        let (project_id, _user_id) = fixtures(&pool).await;
        let repo = SqliteInviteRepository::new(pool.clone());

        let invite = NewInvite {
            token: "anon".to_string(),
            project_id,
            role: Role::Admin,
            email: None,
            created_by: None,
            expires_at: chrono::Utc::now() + Duration::days(1),
        };
        repo.create(invite).await.unwrap();

        let found = repo.find_by_token("anon").await.unwrap().unwrap();
        assert_eq!(found.role, Role::Admin);
        assert!(found.email.is_none());
        assert!(found.created_by.is_none());
    }

    #[tokio::test]
    async fn mark_accepted_and_delete() {
        let pool = test_pool().await;
        let (project_id, user_id) = fixtures(&pool).await;
        let repo = SqliteInviteRepository::new(pool.clone());

        repo.create(new_invite(project_id, user_id, "tok"))
            .await
            .unwrap();

        let when = chrono::Utc::now();
        repo.mark_accepted("tok", when).await.unwrap();
        let found = repo.find_by_token("tok").await.unwrap().unwrap();
        assert!(found.accepted_at.is_some());

        assert_eq!(repo.list_for_project(project_id).await.unwrap().len(), 1);

        repo.delete("tok").await.unwrap();
        assert!(repo.find_by_token("tok").await.unwrap().is_none());
    }
}
