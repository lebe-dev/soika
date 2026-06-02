//! Built-in admin bootstrap (MVP §11).
//!
//! On startup, if `ADMIN_EMAIL` / `ADMIN_PASSWORD` are configured, provision a
//! built-in instance admin. Idempotent: the account is created only when absent,
//! so restarts are safe and the operator's password is never overwritten here.

use crate::config::Config;
use crate::domain::{AuthProvider, User};
use crate::error::{Error, Result};
use crate::ports::{NewUser, UserRepository};

use super::password::hash_password;

/// Outcome of a bootstrap attempt, useful for logging in the bin crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapOutcome {
    /// No `ADMIN_EMAIL`/`ADMIN_PASSWORD` configured — nothing to do.
    Skipped,
    /// A built-in admin already existed; left untouched.
    AlreadyExists,
    /// A new built-in admin account was created.
    Created,
}

/// Idempotently provision the built-in admin from config (§11).
///
/// Returns the outcome so the caller can log it. Errors only on validation
/// (e.g. only one of email/password set) or persistence failures.
pub async fn bootstrap_admin(
    users: &dyn UserRepository,
    config: &Config,
) -> Result<BootstrapOutcome> {
    let (email, password) = match (
        config.admin_email.as_deref(),
        config.admin_password.as_deref(),
    ) {
        (None, None) => return Ok(BootstrapOutcome::Skipped),
        (Some(email), Some(password)) => (email.trim(), password),
        _ => {
            return Err(Error::validation(
                "ADMIN_EMAIL and ADMIN_PASSWORD must both be set to bootstrap the built-in admin",
            ));
        }
    };

    if email.is_empty() {
        return Err(Error::validation("ADMIN_EMAIL must not be empty"));
    }
    if password.is_empty() {
        return Err(Error::validation("ADMIN_PASSWORD must not be empty"));
    }

    if users.find_by_email(email).await?.is_some() {
        return Ok(BootstrapOutcome::AlreadyExists);
    }

    let password_hash = hash_password(password)?;
    let new = NewUser {
        email: email.to_string(),
        display_name: "Administrator".to_string(),
        password_hash,
        is_admin: true,
        auth_provider: AuthProvider::Local,
    };

    // Race note: a UNIQUE(email) conflict here means a concurrent bootstrap won;
    // treat it as already-existing rather than an error.
    match users.create(new).await {
        Ok(_) => Ok(BootstrapOutcome::Created),
        Err(Error::Conflict(_)) => Ok(BootstrapOutcome::AlreadyExists),
        Err(e) => Err(e),
    }
}

/// Verify that `user` is a usable built-in admin (helper for bin logging/checks).
pub fn is_builtin_admin(user: &User) -> bool {
    user.is_admin
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Id, User};
    use crate::ports::UserUpdate;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;

    fn config_with(admin_email: Option<&str>, admin_password: Option<&str>) -> Config {
        Config {
            organization_name: "soika".into(),
            database_url: "sqlite::memory:".into(),
            bind_addr: "127.0.0.1:0".into(),
            base_url: "http://localhost:8080".into(),
            secret_key: "secret".into(),
            allow_signup: false,
            admin_email: admin_email.map(str::to_string),
            admin_password: admin_password.map(str::to_string),
            default_events_retention: 1000,
            default_retention_days: 0,
            retention_cron: "0 */15 * * * *".into(),
            smtp: None,
            oidc: None,
        }
    }

    /// Minimal in-memory user repo for testing bootstrap logic.
    #[derive(Default)]
    struct FakeUsers {
        rows: Mutex<Vec<User>>,
    }

    #[async_trait]
    impl UserRepository for FakeUsers {
        async fn create(&self, new: NewUser) -> Result<User> {
            let mut rows = self.rows.lock().unwrap();
            if rows.iter().any(|u| u.email == new.email) {
                return Err(Error::Conflict("email".into()));
            }
            let user = User {
                id: Id::new_v4(),
                email: new.email,
                display_name: new.display_name,
                password_hash: new.password_hash,
                is_admin: new.is_admin,
                notifications_enabled: true,
                auth_provider: new.auth_provider,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            rows.push(user.clone());
            Ok(user)
        }

        async fn find_by_id(&self, id: Id) -> Result<Option<User>> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.id == id)
                .cloned())
        }

        async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.email == email)
                .cloned())
        }

        async fn update(&self, _id: Id, _update: UserUpdate) -> Result<User> {
            unimplemented!()
        }

        async fn list(&self) -> Result<Vec<User>> {
            Ok(self.rows.lock().unwrap().clone())
        }

        async fn delete(&self, _id: Id) -> Result<()> {
            Ok(())
        }

        async fn count(&self) -> Result<i64> {
            Ok(self.rows.lock().unwrap().len() as i64)
        }
    }

    #[tokio::test]
    async fn skips_when_unconfigured() {
        let users = FakeUsers::default();
        let cfg = config_with(None, None);
        assert_eq!(
            bootstrap_admin(&users, &cfg).await.unwrap(),
            BootstrapOutcome::Skipped
        );
        assert_eq!(users.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn creates_then_is_idempotent() {
        let users = FakeUsers::default();
        let cfg = config_with(Some("admin@example.com"), Some("hunter2"));

        assert_eq!(
            bootstrap_admin(&users, &cfg).await.unwrap(),
            BootstrapOutcome::Created
        );
        assert_eq!(users.count().await.unwrap(), 1);

        // Second run does not create a duplicate.
        assert_eq!(
            bootstrap_admin(&users, &cfg).await.unwrap(),
            BootstrapOutcome::AlreadyExists
        );
        assert_eq!(users.count().await.unwrap(), 1);

        let admin = users
            .find_by_email("admin@example.com")
            .await
            .unwrap()
            .unwrap();
        assert!(admin.is_admin);
        // Password is hashed, not stored in plaintext.
        assert_ne!(admin.password_hash, "hunter2");
    }

    #[tokio::test]
    async fn requires_both_email_and_password() {
        let users = FakeUsers::default();
        let only_email = config_with(Some("a@b.com"), None);
        assert!(matches!(
            bootstrap_admin(&users, &only_email).await.unwrap_err(),
            Error::Validation(_)
        ));

        let only_pw = config_with(None, Some("pw"));
        assert!(matches!(
            bootstrap_admin(&users, &only_pw).await.unwrap_err(),
            Error::Validation(_)
        ));
    }
}
