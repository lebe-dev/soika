use super::Db;
use sqlx::sqlite::SqlitePoolOptions;

/// Build a fresh in-memory SQLite pool with the 0001 schema applied.
///
/// `:memory:` databases are per-connection, so the pool is pinned to a
/// single connection to keep all queries on the same in-memory database.
pub(crate) async fn test_pool() -> Db {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");

    // Enforce FK constraints (SQLite leaves them off by default).
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("enable foreign keys");

    crate::MIGRATOR.run(&pool).await.expect("run migrations");

    pool
}

/// Insert a team row directly (bypassing the team adapter) and return its id.
pub(crate) async fn insert_team(pool: &Db, name: &str) -> crate::domain::Id {
    let id = crate::domain::Id::new_v4();
    let now = super::ts_to_db(chrono::Utc::now());
    sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(super::id_to_db(id))
        .bind(name)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("insert team");
    id
}

/// Insert a project row directly (bypassing the project adapter) and return its id.
pub(crate) async fn insert_project(
    pool: &Db,
    team_id: crate::domain::Id,
    slug: &str,
    dsn: &str,
) -> crate::domain::Id {
    let id = crate::domain::Id::new_v4();
    let now = super::ts_to_db(chrono::Utc::now());
    sqlx::query(
        "INSERT INTO projects (id, short_id, team_id, name, slug, dsn_public_key, \
         retention_events, muted, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 1000, 0, ?, ?)",
    )
    .bind(super::id_to_db(id))
    .bind(super::generate_short_id())
    .bind(super::id_to_db(team_id))
    .bind(slug)
    .bind(slug)
    .bind(dsn)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .expect("insert project");
    id
}

#[tokio::test]
async fn backfill_short_ids_fills_null_rows_with_unique_codes() {
    let pool = test_pool().await;
    let team_id = insert_team(&pool, "t").await;

    // Two projects with NULL short_id (mimicking rows created before
    // migration 0007 added the column).
    let now = super::ts_to_db(chrono::Utc::now());
    for (n, slug) in [("a", "sa"), ("b", "sb")] {
        sqlx::query(
            "INSERT INTO projects (id, team_id, name, slug, dsn_public_key, \
             retention_events, muted, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 1000, 0, ?, ?)",
        )
        .bind(super::id_to_db(crate::domain::Id::new_v4()))
        .bind(super::id_to_db(team_id))
        .bind(n)
        .bind(slug)
        .bind(format!("dsn-{slug}"))
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("insert null-short_id project");
    }

    super::backfill_short_ids(&pool).await.expect("backfill");

    let codes: Vec<String> = sqlx::query("SELECT short_id FROM projects")
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| sqlx::Row::try_get::<String, _>(r, "short_id").unwrap())
        .collect();
    assert_eq!(codes.len(), 2);
    assert!(codes.iter().all(|c| c.len() == 6));
    assert_ne!(codes[0], codes[1], "backfilled codes are unique");

    // Idempotent: a second run leaves the now-filled rows untouched.
    super::backfill_short_ids(&pool)
        .await
        .expect("second backfill");
    let after: Vec<String> = sqlx::query("SELECT short_id FROM projects ORDER BY short_id")
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| sqlx::Row::try_get::<String, _>(r, "short_id").unwrap())
        .collect();
    let mut sorted = codes.clone();
    sorted.sort();
    assert_eq!(after, sorted, "second run is a no-op");
}

/// Insert a user row directly and return its id.
///
/// Leaves `instance_role` at its column default (`'member'`).
pub(crate) async fn insert_user(pool: &Db, email: &str) -> crate::domain::Id {
    let id = crate::domain::Id::new_v4();
    let now = super::ts_to_db(chrono::Utc::now());
    sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash, \
         notifications_enabled, created_at, updated_at) VALUES (?, ?, ?, 'hash', 1, ?, ?)",
    )
    .bind(super::id_to_db(id))
    .bind(email)
    .bind(email)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .expect("insert user");
    id
}
