//! The severity default migration preserves configured classes and delivery settings.
mod common;
use soika::DbPool;

#[tokio::test]
async fn critical_severity_default_migration_preserves_configuration() {
    let database = common::test_db().await;
    macro_rules! check {
        ($pool:expr, $migration:expr, $placeholder:literal) => {
            for severity in ["", "warning", "info", "critical"] {
                sqlx::raw_sql("DROP TABLE notification_settings;
                    CREATE TABLE notification_settings (
                        id INTEGER PRIMARY KEY CHECK (id = 1), config TEXT NOT NULL,
                        refresh_interval_seconds BIGINT NOT NULL DEFAULT 60,
                        default_severity TEXT NOT NULL DEFAULT ''
                    );")
                    .execute($pool).await.unwrap();
                sqlx::query(concat!(
                    "INSERT INTO notification_settings (id, config, refresh_interval_seconds, default_severity) ",
                    "VALUES (1, '{\"alertmanager\":{\"enabled\":true}}', 300, ", $placeholder, ")"
                )).bind(severity).execute($pool).await.unwrap();
                sqlx::raw_sql($migration).execute($pool).await.unwrap();
                let (config, interval, actual): (String, i64, String) = sqlx::query_as(
                    "SELECT config, refresh_interval_seconds, default_severity FROM notification_settings WHERE id = 1")
                    .fetch_one($pool).await.unwrap();
                assert_eq!(config, r#"{"alertmanager":{"enabled":true}}"#);
                assert_eq!(interval, 300);
                assert_eq!(actual, if severity.is_empty() { "critical" } else { severity });
                sqlx::raw_sql("DELETE FROM notification_settings;
                    INSERT INTO notification_settings (id, config) VALUES (1, '{}');")
                    .execute($pool).await.unwrap();
                let default: String = sqlx::query_scalar("SELECT default_severity FROM notification_settings WHERE id = 1")
                    .fetch_one($pool).await.unwrap();
                assert_eq!(default, "critical", "the database column itself defaults to critical");
            }
        };
    }
    match database {
        DbPool::Sqlite(pool) => check!(
            &pool,
            include_str!("../migrations/sqlite/0026_critical_default_severity.sql"),
            "?"
        ),
        DbPool::Postgres(pool) => check!(
            &pool,
            include_str!("../migrations/postgres/0012_critical_default_severity.sql"),
            "$1"
        ),
    }
}
