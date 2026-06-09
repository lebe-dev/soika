//! Favorite project repository port.

use crate::domain::Id;
use crate::error::Result;
use async_trait::async_trait;

/// Per-user list of favorited projects.
#[async_trait]
pub trait FavoriteRepository: Send + Sync {
    async fn add(&self, user_id: Id, project_id: Id) -> Result<()>;
    async fn remove(&self, user_id: Id, project_id: Id) -> Result<()>;
    /// All project ids favorited by this user.
    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Id>>;
}
