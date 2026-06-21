//! SQLite-specific representation helpers, shared by every adapter.
//!
//! SQL is kept ANSI-friendly so a PostgreSQL backend can be added later; the
//! SQLite-specific choices are isolated here:
//!
//!   * UUID ids are stored as TEXT (lowercase hyphenated) — [`id_to_db`]/[`id_from_db`].
//!   * Timestamps are stored as TEXT in RFC3339/ISO-8601 (UTC) — [`ts_to_db`]/[`ts_from_db`].
//!   * Booleans are stored as INTEGER 0/1 — [`bool_to_db`]/[`bool_from_db`].

use crate::domain::{Id, Timestamp};
use crate::error::Error;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

/// Shared handle held by each adapter struct.
pub type Db = SqlitePool;

/// Serialize an [`Id`] (UUID) to its TEXT database form.
pub(crate) fn id_to_db(id: Id) -> String {
    id.to_string()
}

/// Parse an [`Id`] (UUID) from its TEXT database form.
pub(crate) fn id_from_db(s: &str) -> Result<Id, Error> {
    Id::parse_str(s).map_err(|e| Error::internal(format!("invalid uuid in db: {s}: {e}")))
}

/// Serialize a [`Timestamp`] to its TEXT database form (RFC3339, UTC).
pub(crate) fn ts_to_db(ts: Timestamp) -> String {
    ts.to_rfc3339()
}

/// Parse a [`Timestamp`] from its TEXT database form (RFC3339, UTC).
pub(crate) fn ts_from_db(s: &str) -> Result<Timestamp, Error> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| Error::internal(format!("invalid timestamp in db: {s}: {e}")))
}

/// Map an SQLite integer boolean (0/1) to a Rust `bool`.
pub(crate) fn bool_from_db(v: i64) -> bool {
    v != 0
}

/// Map a Rust `bool` to its SQLite integer form (0/1).
pub(crate) fn bool_to_db(v: bool) -> i64 {
    i64::from(v)
}

/// Alphabet for short public ids — lowercase alphanumerics, URL-friendly and
/// unambiguous in a browser address bar (36^6 ≈ 2.1B combinations at length 6).
const SHORT_ID_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Length of a generated short id. Kept small for compact URLs.
const SHORT_ID_LEN: usize = 6;

/// Generate a random short public id (see [`SHORT_ID_ALPHABET`]).
///
/// Callers persist this into a UNIQUE column; the astronomically rare collision
/// is handled by retrying (see [`unique_short_id`]).
pub(crate) fn generate_short_id() -> String {
    (0..SHORT_ID_LEN)
        .map(|_| SHORT_ID_ALPHABET[rand::random_range(0..SHORT_ID_ALPHABET.len())] as char)
        .collect()
}

/// Assign a unique random short id to every project/issue row still missing one.
///
/// Runs once at startup, right after migrations: migration 0007 adds the
/// `short_id` column (leaving existing rows NULL) and the backfill cannot live in
/// SQL because a random value there could collide and abort the UNIQUE-index
/// build. Here each NULL row gets a code from [`unique_short_id`], which retries
/// until it finds a free one — random *and* guaranteed unique. A no-op on a fresh
/// database (and on every subsequent boot once all rows are filled).
pub async fn backfill_short_ids(db: &Db) -> Result<(), Error> {
    for table in ["projects", "issues"] {
        let select = format!("SELECT id FROM {table} WHERE short_id IS NULL LIMIT 1");
        let update = format!("UPDATE {table} SET short_id = ? WHERE id = ?");
        while let Some(row) = sqlx::query(sqlx::AssertSqlSafe(&*select))
            .fetch_optional(db)
            .await?
        {
            let id: String = sqlx::Row::try_get(&row, "id")?;
            let code = unique_short_id(db, table).await?;
            sqlx::query(sqlx::AssertSqlSafe(&*update))
                .bind(code)
                .bind(id)
                .execute(db)
                .await?;
        }
    }
    Ok(())
}

/// Generate a short id not already present in `table.short_id`.
///
/// Pre-checks for a free code (rather than relying on the UNIQUE constraint and
/// distinguishing it from the slug/DSN constraints on the same INSERT). The loop
/// is bounded; with a 2.1-billion keyspace it effectively never iterates twice.
pub(crate) async fn unique_short_id(db: &Db, table: &str) -> Result<String, Error> {
    let sql = format!("SELECT 1 FROM {table} WHERE short_id = ? LIMIT 1");
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

/// Inspect an `INSERT`/`UPDATE` error and classify a UNIQUE-constraint
/// violation as a domain [`Error::Conflict`]. SQLite surfaces these as a
/// `Database` error whose message mentions "UNIQUE constraint failed"; this
/// check is the only SQLite-dialect-aware bit and is isolated here.
pub(crate) fn conflict_or_db(err: sqlx::Error, what: impl Into<String>) -> Error {
    if let sqlx::Error::Database(db_err) = &err {
        let msg = db_err.message().to_ascii_lowercase();
        if msg.contains("unique constraint") || msg.contains("constraint failed") {
            return Error::Conflict(what.into());
        }
    }
    Error::Db(err)
}
