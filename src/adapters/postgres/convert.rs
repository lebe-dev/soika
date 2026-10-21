//! PostgreSQL-specific helpers shared by every adapter.
//!
//! The schema uses native types (`UUID`, `TIMESTAMPTZ`, `BOOLEAN`, `BIGINT`),
//! so ids, timestamps and flags bind and decode directly. What remains here:
//!
//!   * [`ts`] / [`now`] — `TIMESTAMPTZ` stores microseconds, so timestamps are
//!     truncated before they are written; the value an adapter hands back then
//!     equals what a later read returns.
//!   * [`unique_short_id`] — short public ids (see `crate::adapters::short_id`).
//!   * [`conflict_or_db`] — UNIQUE violations (SQLSTATE `23505`) as
//!     [`Error::Conflict`].

use crate::adapters::short_id::generate_short_id;
use crate::domain::Timestamp;
use crate::error::Error;
use chrono::SubsecRound;
use sqlx::PgPool;

/// Shared handle held by each adapter struct.
pub type Db = PgPool;

/// Truncate a timestamp to the microsecond precision of `TIMESTAMPTZ`.
pub(crate) fn ts(ts: Timestamp) -> Timestamp {
    ts.trunc_subsecs(6)
}

/// The current time, at `TIMESTAMPTZ` precision.
pub(crate) fn now() -> Timestamp {
    ts(chrono::Utc::now())
}

/// Generate a short id not already present in `table.short_id`.
///
/// Pre-checks for a free code (rather than relying on the UNIQUE constraint and
/// distinguishing it from the slug/DSN constraints on the same INSERT). The loop
/// is bounded; with a 2.1-billion keyspace it effectively never iterates twice.
pub(crate) async fn unique_short_id(db: &Db, table: &str) -> Result<String, Error> {
    let sql = format!("SELECT 1 FROM {table} WHERE short_id = $1 LIMIT 1");
    for _ in 0..16 {
        let candidate = generate_short_id();
        let taken = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(&candidate)
            .fetch_optional(db)
            .await?
            .is_some();
        if !taken {
            return Ok(candidate);
        }
    }
    Err(Error::internal("could not allocate a unique short id"))
}

/// Classify a UNIQUE-constraint violation as a domain [`Error::Conflict`];
/// every other driver error stays an [`Error::Db`].
pub(crate) fn conflict_or_db(err: sqlx::Error, what: impl Into<String>) -> Error {
    if is_unique_violation(&err) {
        return Error::Conflict(what.into());
    }
    Error::Db(err)
}

/// True when `err` is a UNIQUE-constraint violation.
pub(crate) fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}
