//! Project repository port.

use crate::domain::{Id, Project};
use crate::error::Result;
use async_trait::async_trait;

/// Data required to create a project.
#[derive(Debug, Clone)]
pub struct NewProject {
    pub team_id: Id,
    pub name: String,
    pub slug: String,
    pub dsn_public_key: String,
    pub retention_events: i64,
    /// Age-based retention in days; 0 disables age-based pruning.
    pub retention_days: i64,
    /// Optional per-project webhook URL for notifications.
    pub webhook_url: Option<String>,
}

/// Editable project settings. `None` leaves a field unchanged.
#[derive(Debug, Clone, Default)]
pub struct ProjectUpdate {
    pub name: Option<String>,
    /// Move the project to a different owning team. `None` leaves it unchanged.
    /// The caller must validate the target team exists (the FK is enforced).
    pub team_id: Option<Id>,
    pub retention_events: Option<i64>,
    /// Age-based retention in days; `None` leaves it unchanged.
    pub retention_days: Option<i64>,
    pub muted: Option<bool>,
    /// Per-project webhook URL. Double-`Option` distinguishes the
    /// three partial-update intents: `None` leaves it unchanged,
    /// `Some(Some(url))` sets it, and `Some(None)` clears it to NULL (disabling
    /// the webhook channel). The SQLite adapter writes a CASE-based SET clause so
    /// all three intents are honoured.
    pub webhook_url: Option<Option<String>>,
}

/// CRUD + ingestion/listing lookups for projects.
#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn create(&self, new: NewProject) -> Result<Project>;
    async fn find_by_id(&self, id: Id) -> Result<Option<Project>>;

    /// Resolve a project from its short public id (web URLs / SPA API).
    async fn find_by_short_id(&self, short_id: &str) -> Result<Option<Project>>;

    async fn find_by_slug(&self, slug: &str) -> Result<Option<Project>>;

    /// Resolve a project from a DSN public key — ingestion auth.
    async fn find_by_dsn(&self, dsn_public_key: &str) -> Result<Option<Project>>;

    async fn list(&self) -> Result<Vec<Project>>;
    /// Projects belonging to a team.
    async fn list_for_team(&self, team_id: Id) -> Result<Vec<Project>>;
    /// Projects visible to a user (via membership).
    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Project>>;

    async fn update(&self, id: Id, update: ProjectUpdate) -> Result<Project>;

    /// Rotate the DSN public key (regenerate-dsn).
    async fn regenerate_dsn(&self, id: Id, new_dsn_public_key: String) -> Result<Project>;

    async fn delete(&self, id: Id) -> Result<()>;
}
