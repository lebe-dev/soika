//! User repository port.

use crate::domain::{AuthProvider, Id, InstanceRole, User, UserStatus};
use crate::error::Result;
use async_trait::async_trait;

/// Data required to create a new user.
#[derive(Debug, Clone)]
pub struct NewUser {
    pub email: String,
    pub display_name: String,
    /// Pre-hashed argon2 PHC string. Empty-string sentinel for OIDC accounts.
    pub password_hash: String,
    /// Instance-wide role (`Owner | Manager | Member`).
    pub instance_role: InstanceRole,
    /// Origin of the account: local password or OIDC.
    pub auth_provider: AuthProvider,
    /// Activation status. Local flows pass `Active`; the OIDC admin-approval
    /// flow passes `Pending`.
    pub status: UserStatus,
}

/// Mutable profile fields. `None` leaves the field unchanged.
#[derive(Debug, Clone, Default)]
pub struct UserUpdate {
    pub display_name: Option<String>,
    pub password_hash: Option<String>,
    pub notifications_enabled: Option<bool>,
    /// Activation status; used to approve a pending account.
    pub status: Option<UserStatus>,
}

/// CRUD + lookups for users.
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, new: NewUser) -> Result<User>;
    async fn find_by_id(&self, id: Id) -> Result<Option<User>>;
    async fn find_by_email(&self, email: &str) -> Result<Option<User>>;
    async fn update(&self, id: Id, update: UserUpdate) -> Result<User>;
    /// Set the instance-wide role of a user.
    async fn set_instance_role(&self, id: Id, role: InstanceRole) -> Result<User>;
    async fn list(&self) -> Result<Vec<User>>;
    async fn delete(&self, id: Id) -> Result<()>;
    /// Count of all users (used to gate first-run / bootstrap).
    async fn count(&self) -> Result<i64>;
    /// Count of instance owners (`instance_role = 'owner'`). Drives first-run
    /// detection (the service is considered initialized once at least one Owner
    /// exists) and the last-Owner protection.
    async fn count_owners(&self) -> Result<i64>;
}
