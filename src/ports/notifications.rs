//! Persistent notification configuration, independent of the delivery transport.
use crate::{Result, domain::notifications::NotificationConfig};
use async_trait::async_trait;

#[async_trait]
pub trait NotificationRepository: Send + Sync {
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
