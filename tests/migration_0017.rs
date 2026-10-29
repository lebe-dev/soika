//! Team deletion is protected by the schema, including direct SQL callers.
mod common;
use soika::DbPool;

#[tokio::test]
async fn direct_team_delete_is_restricted_by_foreign_key() {
    let database = common::test_db().await;
    let seed = "INSERT INTO teams(id, name, created_at, updated_at) VALUES ('00000000-0000-0000-0000-000000000001', 'protected', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
INSERT INTO projects(id, short_id, team_id, name, slug, dsn_public_key, created_at, updated_at) VALUES ('00000000-0000-0000-0000-000000000002', 'abc123', '00000000-0000-0000-0000-000000000001', 'keep', 'keep', 'keep-dsn', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');";
    let delete = "DELETE FROM teams WHERE id = '00000000-0000-0000-0000-000000000001'";
    match database {
        DbPool::Sqlite(pool) => {
            sqlx::raw_sql(seed).execute(&pool).await.unwrap();
            let err = sqlx::query(delete).execute(&pool).await.unwrap_err();
            assert!(
                err.as_database_error()
                    .unwrap()
                    .message()
                    .contains("FOREIGN KEY"),
                "{err}"
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects")
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
                1
            );
        }
        DbPool::Postgres(pool) => {
            sqlx::raw_sql(seed).execute(&pool).await.unwrap();
            let err = sqlx::query(delete).execute(&pool).await.unwrap_err();
            assert!(err.as_database_error().unwrap().is_foreign_key_violation());
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects")
                    .fetch_one(&pool)
                    .await
                    .unwrap(),
                1
            );
        }
    }
}
