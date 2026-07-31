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

    /// The database refused a write because it was locked by another writer
    /// (SQLite `SQLITE_BUSY` / `SQLITE_LOCKED`) and the bounded retry budget was
    /// exhausted. Kept distinct from [`Error::Db`] because it is *transient*:
    /// the caller may retry, and ingestion answers it with `429 Retry-After`
    /// instead of a `500` so the SDK re-sends the event rather than dropping it.
    #[error("database is busy: {what}")]
    DbBusy {
        /// What the write was doing (`"upsert issue by fingerprint"`, …).
        what: String,
        /// The last busy error observed, kept so the driver's own diagnosis
        /// stays in the error chain.
        #[source]
        source: Box<Error>,
    },

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

    /// Wrap the last busy error as a [`Error::DbBusy`] once the retry budget for
    /// `what` is spent.
    pub fn db_busy(what: impl Into<String>, source: Error) -> Self {
        Error::DbBusy {
            what: what.into(),
            source: Box::new(source),
        }
    }

    /// True when the error is a *transient* lock conflict the caller may retry:
    /// SQLite `SQLITE_BUSY` (5) / `SQLITE_LOCKED` (6), including their extended
    /// forms (e.g. `SQLITE_BUSY_SNAPSHOT` = 517, returned when a deferred
    /// transaction cannot upgrade its snapshot to a write).
    ///
    /// The classification lives here rather than in the SQLite adapter because
    /// both the adapter's retry loop and the ingest HTTP mapping need it, and the
    /// latter must not depend on an adapter (hexagonal: dependencies point
    /// inward).
    pub fn is_busy(&self) -> bool {
        match self {
            Error::DbBusy { .. } => true,
            Error::Db(sqlx::Error::Database(db_err)) => is_busy_code(db_err.as_ref()),
            _ => false,
        }
    }
}

/// SQLite primary result codes that mean "locked, try again".
const SQLITE_BUSY: i32 = 5;
const SQLITE_LOCKED: i32 = 6;

/// True when a driver error carries a busy/locked result code.
///
/// SQLite reports the *extended* code (`SQLITE_BUSY_SNAPSHOT` = 517 = 5 | 2<<8),
/// whose low byte is the primary code — hence the mask. The message check is a
/// belt-and-braces fallback for drivers that don't surface a numeric code.
fn is_busy_code(db_err: &dyn sqlx::error::DatabaseError) -> bool {
    if let Some(code) = db_err.code().and_then(|c| c.parse::<i32>().ok()) {
        let primary = code & 0xff;
        if primary == SQLITE_BUSY || primary == SQLITE_LOCKED {
            return true;
        }
    }
    let msg = db_err.message().to_ascii_lowercase();
    msg.contains("database is locked") || msg.contains("database table is locked")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    /// Minimal [`sqlx::error::DatabaseError`] stand-in: the real `SqliteError`
    /// cannot be constructed outside the driver, so busy-code classification is
    /// tested against a fake carrying the same code/message shape.
    #[derive(Debug)]
    struct FakeDbError {
        code: Option<&'static str>,
        message: &'static str,
    }

    impl std::fmt::Display for FakeDbError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl std::error::Error for FakeDbError {}

    impl sqlx::error::DatabaseError for FakeDbError {
        fn message(&self) -> &str {
            self.message
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            self.code.map(Cow::Borrowed)
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

    fn db_error(code: Option<&'static str>, message: &'static str) -> Error {
        Error::Db(sqlx::Error::Database(Box::new(FakeDbError {
            code,
            message,
        })))
    }

    #[test]
    fn busy_primary_code_is_busy() {
        assert!(db_error(Some("5"), "database is locked").is_busy());
        assert!(db_error(Some("6"), "database table is locked").is_busy());
    }

    #[test]
    fn busy_extended_codes_are_busy() {
        // SQLITE_BUSY_SNAPSHOT (517) / SQLITE_BUSY_RECOVERY (261) /
        // SQLITE_LOCKED_SHAREDCACHE (262) share the low-byte primary code.
        for code in ["261", "517", "262"] {
            assert!(
                db_error(Some(code), "database is locked").is_busy(),
                "code {code} should classify as busy"
            );
        }
    }

    #[test]
    fn busy_falls_back_to_message_without_a_code() {
        assert!(db_error(None, "database is locked").is_busy());
    }

    #[test]
    fn other_database_errors_are_not_busy() {
        assert!(!db_error(Some("19"), "UNIQUE constraint failed: users.email").is_busy());
        assert!(!Error::Db(sqlx::Error::PoolTimedOut).is_busy());
        assert!(!Error::Validation("bad".into()).is_busy());
    }

    #[test]
    fn db_busy_is_busy_and_keeps_the_cause_in_its_chain() {
        let err = Error::db_busy("insert event", db_error(Some("5"), "database is locked"));
        assert!(err.is_busy());
        let source = std::error::Error::source(&err).expect("source");
        assert!(
            source.to_string().contains("database is locked"),
            "got {source}"
        );
    }
}
