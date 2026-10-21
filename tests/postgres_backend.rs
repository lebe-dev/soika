//! PostgreSQL-only checks against a live server: error classification and
//! migrations. Run with `just test-postgres`; on the default SQLite run every
//! test returns early.

mod common;

use soika::{DbBackend, DbPool, Error};
use sqlx::PgPool;

/// The per-test Postgres pool, or `None` when the run targets SQLite.
async fn postgres() -> Option<PgPool> {
    if common::backend() != DbBackend::Postgres {
        eprintln!("skipped: SOIKA_TEST_DB is not postgres");
        return None;
    }
    match common::test_db().await {
        DbPool::Postgres(pool) => Some(pool),
        DbPool::Sqlite(_) => unreachable!("backend is postgres"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadlock_is_a_transient_busy_error() {
    let Some(pool) = postgres().await else {
        return;
    };
    sqlx::query("CREATE TABLE lock_probe (id INT PRIMARY KEY, v INT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO lock_probe (id, v) VALUES (1, 0), (2, 0)")
        .execute(&pool)
        .await
        .unwrap();

    // Two transactions lock the rows in opposite order; Postgres aborts one of
    // them with SQLSTATE 40P01.
    let mut first = pool.begin().await.unwrap();
    let mut second = pool.begin().await.unwrap();
    sqlx::query("UPDATE lock_probe SET v = 1 WHERE id = 1")
        .execute(&mut *first)
        .await
        .unwrap();
    sqlx::query("UPDATE lock_probe SET v = 2 WHERE id = 2")
        .execute(&mut *second)
        .await
        .unwrap();

    let (a, b) = tokio::join!(
        sqlx::query("UPDATE lock_probe SET v = 1 WHERE id = 2").execute(&mut *first),
        sqlx::query("UPDATE lock_probe SET v = 2 WHERE id = 1").execute(&mut *second),
    );
    let err = match (a, b) {
        (Err(e), _) | (_, Err(e)) => Error::Db(e),
        (Ok(_), Ok(_)) => panic!("expected one transaction to deadlock"),
    };
    assert!(err.is_busy(), "deadlock should be retryable, got {err}");
}

#[tokio::test]
async fn a_unique_violation_is_not_transient() {
    let Some(pool) = postgres().await else {
        return;
    };
    sqlx::query("CREATE TABLE unique_probe (id INT PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO unique_probe (id) VALUES (1)")
        .execute(&pool)
        .await
        .unwrap();

    let err = sqlx::query("INSERT INTO unique_probe (id) VALUES (1)")
        .execute(&pool)
        .await
        .expect_err("duplicate key");
    assert!(!Error::Db(err).is_busy());
}

#[tokio::test]
async fn migrations_can_be_re_run() {
    let Some(pool) = postgres().await else {
        return;
    };
    // `test_db` already migrated; a restart re-runs the migrator.
    DbPool::Postgres(pool.clone())
        .migrate()
        .await
        .expect("re-running migrations is a no-op");

    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(applied, soika::PG_MIGRATOR.iter().count() as i64);
}
