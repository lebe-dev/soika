//! Migration test for `0013_drop_project_memberships.sql` (RBAC Variant A).
//!
//! The riskiest migration folds per-project `memberships` into team membership:
//! no one may lose access, and a user who was project-`Admin` in **any** project
//! of a team must become a Team `Admin` (`Admin -> Team Admin`). Closed
//! membership means access is now strictly team-scoped: a user only gains
//! membership in the teams whose projects they were on — they get no access to
//! unrelated teams' projects.
//!
//! This test reproduces the pre-0013 schema by applying migrations `0001`..=`0012`
//! manually, seeds legacy `memberships` + a project-scoped `invite`, then runs the
//! `0013` SQL and asserts the resulting `team_members` roles and re-scoped invite.
//! It bypasses [`soika::MIGRATOR`] (which would run every migration including
//! 0013) precisely so the *effect* of 0013 on legacy data can be observed.

use soika::MIGRATOR;
use sqlx::Row;
use sqlx::sqlite::SqlitePoolOptions;

/// The 0013 migration version. Everything strictly below is applied first.
const MIGRATION_0013_VERSION: i64 = 13;

/// Apply every migration with `version < before` to the pool, in order, using
/// the embedded migration SQL (multi-statement batches, incl. PRAGMA toggles).
async fn apply_migrations_before(pool: &sqlx::SqlitePool, before: i64) {
    let mut migrations: Vec<_> = MIGRATOR.iter().filter(|m| m.version < before).collect();
    migrations.sort_by_key(|m| m.version);
    for m in migrations {
        sqlx::raw_sql(m.sql.clone())
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("apply migration {}: {e}", m.version));
    }
}

/// Run the 0013 migration SQL against the pool.
async fn apply_migration_0013(pool: &sqlx::SqlitePool) {
    let m = MIGRATOR
        .iter()
        .find(|m| m.version == MIGRATION_0013_VERSION)
        .expect("migration 0013 exists");
    sqlx::raw_sql(m.sql.clone())
        .execute(pool)
        .await
        .expect("apply migration 0013");
}

#[tokio::test]
async fn migration_0013_promotes_project_admin_to_team_admin_and_closes_membership() {
    // A single connection so the in-memory DB persists across statements.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");

    // Build the pre-0013 schema (still has `memberships`, project-scoped invites).
    apply_migrations_before(&pool, MIGRATION_0013_VERSION).await;

    let now = "2026-01-01T00:00:00Z";

    // Two teams, one project each.
    for (tid, name) in [("team-1", "Team One"), ("team-2", "Team Two")] {
        sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(tid)
            .bind(name)
            .bind(now)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (pid, tid, slug, dsn) in [
        ("proj-1", "team-1", "p1", "dsn-1"),
        ("proj-2", "team-2", "p2", "dsn-2"),
    ] {
        sqlx::query(
            "INSERT INTO projects (id, team_id, name, slug, dsn_public_key, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(pid)
        .bind(tid)
        .bind(slug)
        .bind(slug)
        .bind(dsn)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Users (pre-0011 had `is_admin`; it is gone by 0011, so users now carry
    // `instance_role`). All are plain members at the instance level.
    for (uid, email) in [
        ("u-admin", "admin@example.com"),
        ("u-member", "member@example.com"),
        ("u-outsider", "outsider@example.com"),
    ] {
        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, created_at, updated_at) \
             VALUES (?, ?, ?, '', ?, ?)",
        )
        .bind(uid)
        .bind(email)
        .bind(email)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Legacy per-project memberships:
    //  - u-admin is project-ADMIN of proj-1 (team-1)        -> Team Admin of team-1
    //  - u-member is project-MEMBER of proj-1 (team-1)       -> Team Contributor of team-1
    //  - u-outsider is project-ADMIN of proj-2 (team-2)      -> Team Admin of team-2 only
    for (pid, uid, role) in [
        ("proj-1", "u-admin", "admin"),
        ("proj-1", "u-member", "member"),
        ("proj-2", "u-outsider", "admin"),
    ] {
        sqlx::query(
            "INSERT INTO memberships (project_id, user_id, role, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(pid)
        .bind(uid)
        .bind(role)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    }

    // A project-scoped invite that must be re-scoped to the owning team.
    sqlx::query(
        "INSERT INTO invites (token, project_id, role, email, created_by, created_at, expires_at) \
         VALUES ('tok-1', 'proj-1', 'admin', 'invitee@example.com', 'u-admin', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    // --- Run the migration under test. ---
    apply_migration_0013(&pool).await;

    // Helper: read the team role for a (team, user), or None if not a member.
    async fn team_role(pool: &sqlx::SqlitePool, team_id: &str, user_id: &str) -> Option<String> {
        let row = sqlx::query("SELECT role FROM team_members WHERE team_id = ? AND user_id = ?")
            .bind(team_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .unwrap();
        row.map(|r| r.get::<String, _>("role"))
    }

    // Admin -> Team Admin of team-1.
    assert_eq!(
        team_role(&pool, "team-1", "u-admin").await.as_deref(),
        Some("admin"),
        "project-admin is promoted to Team Admin of the owning team"
    );

    // Project member -> Team Contributor of team-1.
    assert_eq!(
        team_role(&pool, "team-1", "u-member").await.as_deref(),
        Some("contributor"),
        "project member becomes a Team Contributor"
    );

    // Closed membership: neither team-1 user gains access to the unrelated
    // team-2 (they had no membership in any of its projects).
    assert_eq!(
        team_role(&pool, "team-2", "u-admin").await,
        None,
        "team-1 admin gains no access to the unrelated team-2"
    );
    assert_eq!(
        team_role(&pool, "team-2", "u-member").await,
        None,
        "team-1 member gains no access to the unrelated team-2"
    );

    // The outsider is a Team Admin of team-2 only — and not of team-1.
    assert_eq!(
        team_role(&pool, "team-2", "u-outsider").await.as_deref(),
        Some("admin"),
        "outsider keeps Team Admin on their own team"
    );
    assert_eq!(
        team_role(&pool, "team-1", "u-outsider").await,
        None,
        "outsider gains no access to the unrelated team-1"
    );

    // The `memberships` table is gone.
    let memberships_exists: i64 = sqlx::query(
        "SELECT COUNT(*) AS c FROM sqlite_master WHERE type='table' AND name='memberships'",
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .get("c");
    assert_eq!(memberships_exists, 0, "memberships table is dropped");

    // The invite is re-scoped from project to its owning team, role mapped
    // admin -> admin (member would map to contributor).
    let invite = sqlx::query("SELECT team_id, role FROM invites WHERE token = 'tok-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        invite.get::<String, _>("team_id"),
        "team-1",
        "invite is re-pointed to the project's owning team"
    );
    assert_eq!(
        invite.get::<String, _>("role"),
        "admin",
        "invite role 'admin' maps to team role 'admin'"
    );
}
