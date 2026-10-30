//! Transport boundary for submitting Alertmanager API alerts.
use crate::{Result, domain::notifications::AlertmanagerAlert};
use async_trait::async_trait;

#[async_trait]
pub trait AlertmanagerClient: Send + Sync {
    async fn send(&self, endpoint: &str, alert: &AlertmanagerAlert) -> Result<()>;
}
