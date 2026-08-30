//! Passkey repository port — WebAuthn credentials bound to a user.

use crate::domain::{Id, Passkey, Timestamp};
use crate::error::Result;
use async_trait::async_trait;

/// Data required to store a freshly registered credential.
#[derive(Debug, Clone)]
pub struct NewPasskey {
    pub user_id: Id,
    /// Raw credential id, URL-safe base64 (no padding).
    pub credential_id: String,
    pub name: String,
    /// Opaque credential serialization owned by `crate::auth::passkey`.
    pub credential: String,
}

/// CRUD + lookups for passkeys.
#[async_trait]
pub trait PasskeyRepository: Send + Sync {
    /// Store a newly registered credential. A `credential_id` already known to
    /// the instance is a [`crate::error::Error::Conflict`].
    async fn create(&self, new: NewPasskey) -> Result<Passkey>;
    /// All credentials of a user, newest first.
    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Passkey>>;
    /// Look up a single credential by its raw (base64url) credential id.
    async fn find_by_credential_id(&self, credential_id: &str) -> Result<Option<Passkey>>;
    /// Look up a credential owned by `user_id` (management operations).
    async fn find_for_user(&self, user_id: Id, id: Id) -> Result<Option<Passkey>>;
    /// Rename a credential owned by `user_id`.
    async fn rename(&self, user_id: Id, id: Id, name: &str) -> Result<Passkey>;
    /// Persist the post-assertion credential state (counter, backup flags) and
    /// stamp `last_used_at`.
    async fn touch(&self, id: Id, credential: &str, used_at: Timestamp) -> Result<()>;
    /// Delete a credential owned by `user_id`.
    async fn delete(&self, user_id: Id, id: Id) -> Result<()>;
    /// Number of credentials registered by a user (quota enforcement).
    async fn count_for_user(&self, user_id: Id) -> Result<i64>;
}
