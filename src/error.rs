//! Library error type (the lib crate uses `thiserror`).
//!
//! HTTP mapping lives in `crate::api` / `crate::router`; this enum is the single
//! domain-level error currency returned by ports and services.

use thiserror::Error;

/// The result alias used throughout the library crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All error conditions the library surfaces to callers.
#[derive(Debug, Error)]
pub enum Error {
    /// Requested entity does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// Input failed validation.
    #[error("validation error: {0}")]
    Validation(String),

    /// Authentication or authorization failed.
    #[error("authentication error: {0}")]
    Auth(String),

    /// Caller lacks permission for the action.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// Uniqueness / state conflict (e.g. duplicate email, slug).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Rate limit exceeded — maps to HTTP 429.
    #[error("rate limited")]
    RateLimited,

    /// Database / persistence failure.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    /// JSON (de)serialization failure.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// Outbound email failure.
    #[error("mail error: {0}")]
    Mail(String),

    /// Email template loading or rendering failure.
    #[error("template error: {0}")]
    Template(String),

    /// Catch-all internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Convenience constructor for `NotFound`.
    pub fn not_found(what: impl Into<String>) -> Self {
        Error::NotFound(what.into())
    }

    /// Convenience constructor for `Validation`.
    pub fn validation(msg: impl Into<String>) -> Self {
        Error::Validation(msg.into())
    }

    /// Convenience constructor for `Internal`.
    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }
}
