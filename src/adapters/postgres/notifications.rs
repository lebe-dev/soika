use super::Db;
use crate::{
    Result, adapters::retry::retry_write, config::WritePolicy,
    domain::notifications::NotificationConfig, ports::NotificationRepository,
};
use async_trait::async_trait;

pub struct PgNotificationRepository {
    db: Db,
}
impl PgNotificationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}
#[async_trait]
impl NotificationRepository for PgNotificationRepository {
    async fn severity(
        &self,
        project_id: Option<crate::domain::Id>,
    ) -> Result<Option<crate::domain::notifications::AlertSeverity>> {
        let value: String = if let Some(id) = project_id {
            sqlx::query_scalar("SELECT alertmanager_severity FROM projects WHERE id = $1")
                .bind(id)
                .fetch_one(&self.db)
                .await?
        } else {
            sqlx::query_scalar("SELECT default_severity FROM notification_settings WHERE id = 1")
                .fetch_one(&self.db)
                .await?
        };
        if value.is_empty() {
            return Ok(None);
        }
        Ok(Some(value.parse()?))
    }
    async fn set_severity(
        &self,
        project_id: Option<crate::domain::Id>,
        severity: Option<crate::domain::notifications::AlertSeverity>,
    ) -> Result<()> {
        let value = severity.map_or("", |value| value.as_str());
        retry_write(
            &WritePolicy::default(),
            "set project alert severity",
            || async {
                if let Some(id) = project_id {
                    sqlx::query("UPDATE projects SET alertmanager_severity = $1 WHERE id = $2")
                        .bind(value)
                        .bind(id)
                        .execute(&self.db)
                        .await?;
                } else {
                    sqlx::query(
                        "UPDATE notification_settings SET default_severity = $1 WHERE id = 1",
                    )
                    .bind(value)
                    .execute(&self.db)
                    .await?;
                }
                Ok(())
            },
        )
        .await
    }

    async fn interval(
        &self,
        project_id: Option<crate::domain::Id>,
    ) -> Result<crate::domain::notifications::AlertmanagerInterval> {
        let global: i64 = sqlx::query_scalar(
            "SELECT refresh_interval_seconds FROM notification_settings WHERE id = 1",
        )
        .fetch_one(&self.db)
        .await?;
        let seconds = if let Some(id) = project_id {
            let value: i64 = sqlx::query_scalar(
                "SELECT alertmanager_interval_seconds FROM projects WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&self.db)
            .await?;
            (value != 0).then_some(value)
        } else {
            Some(global)
        };
        Ok(crate::domain::notifications::AlertmanagerInterval {
            interval_seconds: seconds,
            effective_interval_seconds: seconds.unwrap_or(global),
        })
    }
    async fn set_interval(
        &self,
        project_id: Option<crate::domain::Id>,
        seconds: Option<i64>,
    ) -> Result<()> {
        if let Some(seconds) = seconds {
            crate::domain::notifications::validate_interval(seconds)?;
        }
        if project_id.is_none() && seconds.is_none() {
            return Err(crate::Error::validation("global interval is required"));
        }
        retry_write(&WritePolicy::default(), "set Alertmanager interval", || async {
            if let Some(id) = project_id {
                sqlx::query("UPDATE projects SET alertmanager_interval_seconds = $1 WHERE id = $2")
                    .bind(seconds.unwrap_or(0)).bind(id).execute(&self.db).await?;
            } else {
                sqlx::query("UPDATE notification_settings SET refresh_interval_seconds = $1 WHERE id = 1")
                    .bind(seconds).execute(&self.db).await?;
            }
            Ok(())
        }).await
    }

    async fn find_alert(
        &self,
        issue_id: crate::domain::Id,
    ) -> Result<Option<crate::domain::notifications::ActiveAlert>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT payload FROM notification_alerts WHERE issue_id = $1")
                .bind(issue_id.to_string())
                .fetch_optional(&self.db)
                .await?;
        value
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    async fn active(
        &self,
        after: &str,
        limit: i64,
    ) -> Result<Vec<crate::domain::notifications::ActiveAlert>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT payload FROM notification_alerts WHERE issue_id > $1 ORDER BY issue_id LIMIT $2")
            .bind(after).bind(limit).fetch_all(&self.db).await?;
        rows.iter()
            .map(|value| serde_json::from_str(value).map_err(Into::into))
            .collect()
    }
    async fn save_alert(&self, alert: &crate::domain::notifications::ActiveAlert) -> Result<()> {
        let value = serde_json::to_string(alert)?;
        retry_write(&WritePolicy::default(), "save notification alert", || async {
            sqlx::query("INSERT INTO notification_alerts (issue_id, payload) VALUES ($1, $2) ON CONFLICT (issue_id) DO UPDATE SET payload = excluded.payload")
                .bind(alert.issue_id.to_string()).bind(&value).execute(&self.db).await?;
            Ok(())
        }).await
    }
    async fn remove_alert(&self, issue_id: crate::domain::Id) -> Result<()> {
        retry_write(
            &WritePolicy::default(),
            "remove notification alert",
            || async {
                sqlx::query("DELETE FROM notification_alerts WHERE issue_id = $1")
                    .bind(issue_id.to_string())
                    .execute(&self.db)
                    .await?;
                Ok(())
            },
        )
        .await
    }
    async fn get(&self) -> Result<NotificationConfig> {
        let value: String =
            sqlx::query_scalar("SELECT config FROM notification_settings WHERE id = 1")
                .fetch_one(&self.db)
                .await?;
        Ok(serde_json::from_str(&value)?)
    }
    async fn set(&self, config: &NotificationConfig) -> Result<()> {
        config.validate_stored()?;
        let value = serde_json::to_string(config)?;
        retry_write(
            &WritePolicy::default(),
            "set notification settings",
            || async {
                sqlx::query("UPDATE notification_settings SET config = $1 WHERE id = 1")
                    .bind(&value)
                    .execute(&self.db)
                    .await?;
                Ok(())
            },
        )
        .await
    }
}
