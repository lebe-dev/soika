//! SQLite [`TeamRepository`] adapter.

use super::{Db, bool_from_db, conflict_or_db, id_from_db, id_to_db, ts_from_db, ts_to_db};
use crate::domain::{Id, Team, TeamMember, Timestamp, User};
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
}

#[derive(FromRow)]
struct TeamRow {
    id: String,
    name: String,
    created_at: String,
    updated_at: String,
}

impl TeamRow {
    fn into_domain(self) -> Result<Team> {
        Ok(Team {
            id: id_from_db(&self.id)?,
            name: self.name,
            created_at: ts_from_db(&self.created_at)?,
            updated_at: ts_from_db(&self.updated_at)?,
        })
    }
}

/// Row used when joining `team_members` with `users` for [`TeamRepository::members`].
#[derive(FromRow)]
struct MemberUserRow {
    id: String,
    email: String,
    display_name: String,
    password_hash: String,
    is_admin: i64,
    notifications_enabled: i64,
    created_at: String,
    updated_at: String,
}

impl MemberUserRow {
    fn into_domain(self) -> Result<User> {
        Ok(User {
            id: id_from_db(&self.id)?,
            email: self.email,
            display_name: self.display_name,
            password_hash: self.password_hash,
            is_admin: bool_from_db(self.is_admin),
            notifications_enabled: bool_from_db(self.notifications_enabled),
            created_at: ts_from_db(&self.created_at)?,
            updated_at: ts_from_db(&self.updated_at)?,
        })
    }
}

const SELECT_TEAM: &str = "SELECT id, name, created_at, updated_at FROM teams";

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
            id,
            name,
            created_at: now,
            updated_at: now,
        })
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Team>> {
        let row = sqlx::query_as::<_, TeamRow>(&format!("{SELECT_TEAM} WHERE id = ?"))
            .bind(id_to_db(id))
            .fetch_optional(&self.db)
            .await?;
        row.map(TeamRow::into_domain).transpose()
    }

    async fn list(&self) -> Result<Vec<Team>> {
        let rows = sqlx::query_as::<_, TeamRow>(&format!("{SELECT_TEAM} ORDER BY name"))
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

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM teams WHERE id = ?")
            .bind(id_to_db(id))
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Team>> {
        let rows = sqlx::query_as::<_, TeamRow>(
            "SELECT t.id, t.name, t.created_at, t.updated_at FROM teams t \
             JOIN team_members m ON m.team_id = t.id \
             WHERE m.user_id = ? ORDER BY t.name",
        )
        .bind(id_to_db(user_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(TeamRow::into_domain).collect()
    }

    async fn add_member(&self, team_id: Id, user_id: Id) -> Result<TeamMember> {
        let now: Timestamp = chrono::Utc::now();
        // Idempotent join: ignore a duplicate (team_id, user_id) pair.
        sqlx::query(
            "INSERT INTO team_members (team_id, user_id, created_at) VALUES (?, ?, ?) \
             ON CONFLICT (team_id, user_id) DO NOTHING",
        )
        .bind(id_to_db(team_id))
        .bind(id_to_db(user_id))
        .bind(ts_to_db(now))
        .execute(&self.db)
        .await?;

        Ok(TeamMember {
            team_id,
            user_id,
            created_at: now,
        })
    }

    async fn remove_member(&self, team_id: Id, user_id: Id) -> Result<()> {
        sqlx::query("DELETE FROM team_members WHERE team_id = ? AND user_id = ?")
            .bind(id_to_db(team_id))
            .bind(id_to_db(user_id))
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn members(&self, team_id: Id) -> Result<Vec<User>> {
        let rows = sqlx::query_as::<_, MemberUserRow>(
            "SELECT u.id, u.email, u.display_name, u.password_hash, u.is_admin, \
             u.notifications_enabled, u.created_at, u.updated_at \
             FROM users u JOIN team_members m ON m.user_id = u.id \
             WHERE m.team_id = ? ORDER BY u.display_name",
        )
        .bind(id_to_db(team_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(MemberUserRow::into_domain).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::{insert_user, test_pool};

    #[tokio::test]
    async fn create_rename_list_delete() {
        let pool = test_pool().await;
        let repo = SqliteTeamRepository::new(pool.clone());

        let t = repo.create("Backend".to_string()).await.unwrap();
        assert_eq!(repo.list().await.unwrap().len(), 1);

        let renamed = repo.rename(t.id, "Platform".to_string()).await.unwrap();
        assert_eq!(renamed.name, "Platform");
        assert_eq!(renamed.created_at, t.created_at);

        repo.delete(t.id).await.unwrap();
        assert!(repo.find_by_id(t.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn duplicate_name_is_conflict() {
        let pool = test_pool().await;
        let repo = SqliteTeamRepository::new(pool.clone());
        repo.create("Dup".to_string()).await.unwrap();
        let err = repo.create("Dup".to_string()).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn membership_add_remove_and_listing() {
        let pool = test_pool().await;
        let repo = SqliteTeamRepository::new(pool.clone());

        let team = repo.create("T".to_string()).await.unwrap();
        let u1 = insert_user(&pool, "m1@example.com").await;
        let u2 = insert_user(&pool, "m2@example.com").await;

        repo.add_member(team.id, u1).await.unwrap();
        repo.add_member(team.id, u2).await.unwrap();
        // Idempotent re-add does not error or duplicate.
        repo.add_member(team.id, u1).await.unwrap();

        let members = repo.members(team.id).await.unwrap();
        assert_eq!(members.len(), 2);

        let teams_for_u1 = repo.list_for_user(u1).await.unwrap();
        assert_eq!(teams_for_u1.len(), 1);
        assert_eq!(teams_for_u1[0].id, team.id);

        repo.remove_member(team.id, u1).await.unwrap();
        assert_eq!(repo.members(team.id).await.unwrap().len(), 1);
        assert!(repo.list_for_user(u1).await.unwrap().is_empty());
    }
}
