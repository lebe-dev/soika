//! Service settings repository port.

use crate::domain::{Id, ServiceSettings};
use crate::error::Result;
use async_trait::async_trait;

/// Read/update the singleton instance-wide settings row.
#[async_trait]
pub trait SettingsRepository: Send + Sync {
    /// Fetch the current settings (always present after migration).
    async fn get(&self) -> Result<ServiceSettings>;
    /// Teams assigned to newly created accounts; empty until configured.
    async fn default_team_ids(&self) -> Result<Vec<Id>>;
    /// Atomically replace defaults, rejecting unknown teams. Never changes memberships.
    async fn set_default_team_ids(&self, team_ids: Vec<Id>) -> Result<()>;
    /// Toggle public self-registration.
    async fn set_allow_signup(&self, allow: bool) -> Result<ServiceSettings>;
    /// Update the organization display name.
    async fn set_org_name(&self, org_name: String) -> Result<ServiceSettings>;
}
