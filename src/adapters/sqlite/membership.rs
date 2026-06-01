//! SQLite [`MembershipRepository`] adapter.

use super::{bool_from_db, id_from_db, id_to_db, ts_from_db, ts_to_db, Db};
use crate::domain::{Id, Membership, Role, Timestamp, User};
use crate::error::{Error, Result};
use crate::ports::MembershipRepository;
use async_trait::async_trait;
use sqlx::FromRow;
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteMembershipRepository {
    db: Db,
}

impl SqliteMembershipRepository {
    pub fn new(db: Db) -> Self {
        SqliteMembershipRepository { db }
    }
}

#[derive(FromRow)]
struct MembershipRow {
    project_id: String,
    user_id: String,
    role: String,
    created_at: String,
}

impl MembershipRow {
    fn into_domain(self) -> Result<Membership> {
        Ok(Membership {
            project_id: id_from_db(&self.project_id)?,
            user_id: id_from_db(&self.user_id)?,
            role: Role::from_str(&self.role)?,
            created_at: ts_from_db(&self.created_at)?,
        })
    }
}

/// Join of `memberships` with `users` for [`MembershipRepository::members`].
#[derive(FromRow)]
struct MemberRow {
    id: String,
    email: String,
    display_name: String,
    password_hash: String,
    is_admin: i64,
    notifications_enabled: i64,
    created_at: String,
    updated_at: String,
    role: String,
}

impl MemberRow {
    fn into_domain(self) -> Result<(User, Role)> {
        let user = User {
            id: id_from_db(&self.id)?,
            email: self.email,
            display_name: self.display_name,
            password_hash: self.password_hash,
            is_admin: bool_from_db(self.is_admin),
            notifications_enabled: bool_from_db(self.notifications_enabled),
            created_at: ts_from_db(&self.created_at)?,
            updated_at: ts_from_db(&self.updated_at)?,
        };
        Ok((user, Role::from_str(&self.role)?))
    }
}

const SELECT_MEMBERSHIP: &str = "SELECT project_id, user_id, role, created_at FROM memberships";

#[async_trait]
impl MembershipRepository for SqliteMembershipRepository {
    async fn upsert(&self, project_id: Id, user_id: Id, role: Role) -> Result<Membership> {
        let now: Timestamp = chrono::Utc::now();
        // Insert, or update the role of an existing (project, user) membership.
        // created_at is preserved on update.
        sqlx::query(
            "INSERT INTO memberships (project_id, user_id, role, created_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT (project_id, user_id) DO UPDATE SET role = excluded.role",
        )
        .bind(id_to_db(project_id))
        .bind(id_to_db(user_id))
        .bind(role.as_str())
        .bind(ts_to_db(now))
        .execute(&self.db)
        .await?;

        // Re-read to return the authoritative created_at.
        self.find(project_id, user_id)
            .await?
            .ok_or_else(|| Error::internal("membership vanished after upsert"))
    }

    async fn find(&self, project_id: Id, user_id: Id) -> Result<Option<Membership>> {
        let row = sqlx::query_as::<_, MembershipRow>(&format!(
            "{SELECT_MEMBERSHIP} WHERE project_id = ? AND user_id = ?"
        ))
        .bind(id_to_db(project_id))
        .bind(id_to_db(user_id))
        .fetch_optional(&self.db)
        .await?;
        row.map(MembershipRow::into_domain).transpose()
    }

    async fn remove(&self, project_id: Id, user_id: Id) -> Result<()> {
        sqlx::query("DELETE FROM memberships WHERE project_id = ? AND user_id = ?")
            .bind(id_to_db(project_id))
            .bind(id_to_db(user_id))
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn list_for_project(&self, project_id: Id) -> Result<Vec<Membership>> {
        let rows = sqlx::query_as::<_, MembershipRow>(&format!(
            "{SELECT_MEMBERSHIP} WHERE project_id = ? ORDER BY created_at"
        ))
        .bind(id_to_db(project_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(MembershipRow::into_domain).collect()
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Membership>> {
        let rows = sqlx::query_as::<_, MembershipRow>(&format!(
            "{SELECT_MEMBERSHIP} WHERE user_id = ? ORDER BY created_at"
        ))
        .bind(id_to_db(user_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(MembershipRow::into_domain).collect()
    }

    async fn members(&self, project_id: Id) -> Result<Vec<(User, Role)>> {
        let rows = sqlx::query_as::<_, MemberRow>(
            "SELECT u.id, u.email, u.display_name, u.password_hash, u.is_admin, \
             u.notifications_enabled, u.created_at, u.updated_at, m.role \
             FROM users u JOIN memberships m ON m.user_id = u.id \
             WHERE m.project_id = ? ORDER BY u.display_name",
        )
        .bind(id_to_db(project_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(MemberRow::into_domain).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::{insert_project, insert_team, insert_user, test_pool};

    async fn fixtures(pool: &Db) -> (Id, Id) {
        let team_id = insert_team(pool, "T").await;
        let project_id = insert_project(pool, team_id, "p", "dsnkey").await;
        let user_id = insert_user(pool, "u@example.com").await;
        (project_id, user_id)
    }

    #[tokio::test]
    async fn upsert_changes_role_in_place() {
        let pool = test_pool().await;
        let (project_id, user_id) = fixtures(&pool).await;
        let repo = SqliteMembershipRepository::new(pool.clone());

        let first = repo
            .upsert(project_id, user_id, Role::Member)
            .await
            .unwrap();
        assert_eq!(first.role, Role::Member);

        let promoted = repo.upsert(project_id, user_id, Role::Admin).await.unwrap();
        assert_eq!(promoted.role, Role::Admin);
        // created_at preserved across the upsert.
        assert_eq!(promoted.created_at, first.created_at);

        // Only one membership row exists.
        assert_eq!(repo.list_for_project(project_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn find_remove_and_member_join() {
        let pool = test_pool().await;
        let (project_id, user_id) = fixtures(&pool).await;
        let repo = SqliteMembershipRepository::new(pool.clone());

        assert!(repo.find(project_id, user_id).await.unwrap().is_none());
        repo.upsert(project_id, user_id, Role::Admin).await.unwrap();

        let found = repo.find(project_id, user_id).await.unwrap().unwrap();
        assert_eq!(found.role, Role::Admin);

        let members = repo.members(project_id).await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].0.id, user_id);
        assert_eq!(members[0].1, Role::Admin);

        assert_eq!(repo.list_for_user(user_id).await.unwrap().len(), 1);

        repo.remove(project_id, user_id).await.unwrap();
        assert!(repo.find(project_id, user_id).await.unwrap().is_none());
    }
}
