//! User repository port (MVP §10, §11).

use crate::domain::{AuthProvider, Id, User};
use crate::error::Result;
use async_trait::async_trait;

/// Data required to create a new user.
#[derive(Debug, Clone)]
pub struct NewUser {
    pub email: String,
    pub display_name: String,
    /// Pre-hashed argon2 PHC string. Empty-string sentinel for OIDC accounts.
    pub password_hash: String,
    pub is_admin: bool,
    /// Origin of the account: local password or OIDC (PLAN §5).
    pub auth_provider: AuthProvider,
}

/// Mutable profile fields (MVP §10.3). `None` leaves the field unchanged.
#[derive(Debug, Clone, Default)]
pub struct UserUpdate {
    pub display_name: Option<String>,
    pub password_hash: Option<String>,
    pub notifications_enabled: Option<bool>,
}

/// CRUD + lookups for users.
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, new: NewUser) -> Result<User>;
    async fn find_by_id(&self, id: Id) -> Result<Option<User>>;
    async fn find_by_email(&self, email: &str) -> Result<Option<User>>;
    async fn update(&self, id: Id, update: UserUpdate) -> Result<User>;
    async fn list(&self) -> Result<Vec<User>>;
    async fn delete(&self, id: Id) -> Result<()>;
    /// Count of all users (used to gate first-run / bootstrap, §11).
    async fn count(&self) -> Result<i64>;
}
