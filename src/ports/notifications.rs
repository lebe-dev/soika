//! Persistent notification configuration, independent of the delivery transport.
use crate::{Result, domain::notifications::NotificationConfig};
use async_trait::async_trait;

#[async_trait]
pub trait NotificationRepository: Send + Sync {
    async fn severity(
        &self,
        project_id: Option<crate::domain::Id>,
    ) -> Result<Option<crate::domain::notifications::AlertSeverity>>;
    async fn set_severity(
        &self,
        project_id: Option<crate::domain::Id>,
        severity: Option<crate::domain::notifications::AlertSeverity>,
    ) -> Result<()>;
    async fn interval(
        &self,
        project_id: Option<crate::domain::Id>,
    ) -> Result<crate::domain::notifications::AlertmanagerInterval>;
    /// None resets a project to inheritance; the global value must be present.
    async fn set_interval(
        &self,
        project_id: Option<crate::domain::Id>,
        seconds: Option<i64>,
    ) -> Result<()>;
    async fn find_alert(
        &self,
        issue_id: crate::domain::Id,
    ) -> Result<Option<crate::domain::notifications::ActiveAlert>>;
    async fn active(
        &self,
        after: &str,
        limit: i64,
    ) -> Result<Vec<crate::domain::notifications::ActiveAlert>>;
    async fn save_alert(&self, alert: &crate::domain::notifications::ActiveAlert) -> Result<()>;
    async fn remove_alert(&self, issue_id: crate::domain::Id) -> Result<()>;
    async fn get(&self) -> Result<NotificationConfig>;
    async fn set(&self, config: &NotificationConfig) -> Result<()>;
}
