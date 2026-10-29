//! One-use, server-side authentication ceremony storage.
use crate::{Result, domain::Timestamp};
use async_trait::async_trait;

#[async_trait]
pub trait AuthChallengeRepository: Send + Sync {
    /// Persist a random opaque handle with its purpose, serialized state and expiry.
    async fn create(
        &self,
        id: &str,
        purpose: &str,
        state: &str,
        expires_at: Timestamp,
    ) -> Result<()>;
    /// Atomically consume an unexpired handle, including under concurrent requests.
    async fn take(&self, id: &str, purpose: &str, now: Timestamp) -> Result<Option<String>>;
}
