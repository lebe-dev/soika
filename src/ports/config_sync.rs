//! Atomic configuration reconciliation port.
use crate::{
    Result,
    domain::{
        Id,
        config_file::{ConfigFile, ManagementSettings, SyncReport},
    },
};
use async_trait::async_trait;
#[async_trait]
pub trait ConfigSync: Send + Sync {
    async fn apply(
        &self,
        desired: &ConfigFile,
        settings: &ManagementSettings,
        retention: i64,
    ) -> Result<SyncReport>;
    /// Save the verified IdP group snapshot and reconcile memberships atomically.
    async fn inspect_team(&self, team: Id) -> Result<crate::domain::config_file::TeamGrantView>;
    async fn reconcile_user(&self, user: Id, groups: &[String]) -> Result<bool>;
}
