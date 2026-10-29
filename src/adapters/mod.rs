//! Infrastructure adapters implementing the [`crate::ports`] traits.

pub mod clock;
pub(crate) mod config_sync;
pub mod mailer;
pub mod postgres;
pub(crate) mod retry;
pub(crate) mod short_id;
pub mod sqlite;
