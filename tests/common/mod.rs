//! Shared integration-test harness: a fresh, migrated database per test on the
//! backend chosen by `SOIKA_TEST_DB`.
//!
//!   * unset / `sqlite` — an in-memory SQLite database (no external services);
//!   * `postgres` — PostgreSQL 15 in a Docker container started through
//!     testcontainers. The container is named and marked reusable, so every
//!     test binary of a run shares it (`just test-postgres` removes it at the
//!     end); each test gets its own freshly created database inside it.

use std::sync::OnceLock;

use soika::config::DbConfig;
use soika::{DbBackend, DbPool};
use sqlx::{Connection, Executor, PgConnection};
use testcontainers::{ImageExt, ReuseDirective, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

/// Image tag of the PostgreSQL server the suite runs against.
const POSTGRES_TAG: &str = "15-alpine";

/// Name of the shared, reusable test container.
pub const POSTGRES_CONTAINER: &str = "soika-test-postgres";

/// The backend selected for this test run (`SOIKA_TEST_DB`).
pub fn backend() -> DbBackend {
    match std::env::var("SOIKA_TEST_DB").as_deref() {
        Ok("postgres") => DbBackend::Postgres,
        Ok("sqlite") | Err(_) => DbBackend::Sqlite,
        Ok(other) => {
            panic!("SOIKA_TEST_DB: unknown backend {other:?} (expected sqlite or postgres)")
        }
    }
}

/// A fresh database on the selected backend with all migrations applied.
pub async fn test_db() -> DbPool {
    match backend() {
        DbBackend::Sqlite => sqlite_db().await,
        DbBackend::Postgres => postgres_db().await,
    }
}

async fn sqlite_db() -> DbPool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect in-memory sqlite");
    let pool = DbPool::Sqlite(pool);
    pool.migrate().await.expect("run sqlite migrations");
    pool
}

async fn postgres_db() -> DbPool {
    let server = postgres_server_url();
    let name = format!("soika_test_{}", uuid::Uuid::new_v4().simple());

    let mut admin = PgConnection::connect(&format!("{server}/postgres"))
        .await
        .expect("connect to the postgres test server");
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .await
        .expect("create the per-test database");
    admin.close().await.ok();

    let pool = soika::connect_pg_pool(&format!("{server}/{name}"), &DbConfig::default())
        .await
        .expect("connect to the per-test database");
    let pool = DbPool::Postgres(pool);
    pool.migrate().await.expect("run postgres migrations");
    pool
}

/// Base URL (no database) of the shared Postgres test server, starting the
/// container on first use.
///
/// Every `#[tokio::test]` has its own runtime, so the container is started on a
/// dedicated thread with its own runtime and only the URL is kept. Being marked
/// reusable, the container is not removed when that handle drops.
fn postgres_server_url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| {
        std::thread::spawn(start_postgres)
            .join()
            .expect("start the postgres test container")
    })
}

fn start_postgres() -> String {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a runtime for the container start");
    runtime.block_on(async {
        let container = Postgres::default()
            .with_tag(POSTGRES_TAG)
            .with_container_name(POSTGRES_CONTAINER)
            .with_reuse(ReuseDirective::Always)
            // Durability is irrelevant for throwaway test data; parallel tests
            // each hold a small pool.
            .with_cmd([
                "-c",
                "fsync=off",
                "-c",
                "synchronous_commit=off",
                "-c",
                "full_page_writes=off",
                "-c",
                "max_connections=500",
            ])
            .start()
            .await
            .expect("start postgres container (is Docker running?)");
        let host = container.get_host().await.expect("container host");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("container port");
        format!("postgres://postgres:postgres@{host}:{port}")
    })
}
