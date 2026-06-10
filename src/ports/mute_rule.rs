//! Project-level tag-mute rule repository port.

use crate::domain::{Id, TagMatch, TagMuteRule};
use crate::error::Result;
use async_trait::async_trait;

/// Inputs for creating a [`TagMuteRule`]. `tags` is validated non-empty by the
/// API before reaching the repository.
#[derive(Debug, Clone)]
pub struct NewTagMuteRule {
    pub project_id: Id,
    pub name: Option<String>,
    pub created_by: Option<Id>,
    pub tags: Vec<TagMatch>,
}

/// CRUD over a project's tag-mute rules.
#[async_trait]
pub trait TagMuteRuleRepository: Send + Sync {
    /// Persist a new rule with its tag pairs; returns the stored rule.
    async fn create(&self, new: NewTagMuteRule) -> Result<TagMuteRule>;
    /// All rules for a project (with their tags), newest first.
    async fn list_for_project(&self, project_id: Id) -> Result<Vec<TagMuteRule>>;
    /// Delete a rule scoped to its project. Returns `true` if a row was removed.
    async fn delete(&self, project_id: Id, rule_id: Id) -> Result<bool>;
}
