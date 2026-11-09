//! Persistent Android build artifacts. Chunk keys are SHA-1 checksums.
use crate::{Result, domain::Id};
use async_trait::async_trait;
#[async_trait]
pub trait DebugFileRepository: Send + Sync {
    async fn put(&self, owner: Id, kind: &str, key: &str, data: &str) -> Result<()>;
    async fn get(&self, owner: Id, kind: &str, key: &str) -> Result<Option<String>>;
}
