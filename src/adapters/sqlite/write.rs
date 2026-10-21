//! Write-path helpers shared by the SQLite adapters: explicit write
//! transactions and bounded retries on lock contention.
//!
//! SQLite allows exactly **one writer** for the whole database file, so a
//! single-binary deployment ingesting events concurrently *will* hit lock
//! contention. Two rules keep that contention from turning into failed writes:
//!
//! 1. **Write transactions must be `BEGIN IMMEDIATE`** ([`begin_write`]).
//!    sqlx's `Pool::begin` issues a plain `BEGIN`, which is *deferred*: the
//!    transaction starts as a reader and only asks for the write lock at its
//!    first `INSERT`/`UPDATE`. If another connection committed in between, that
//!    upgrade cannot succeed — SQLite returns `SQLITE_BUSY` (`database is
//!    locked`) **immediately**, and `busy_timeout` does not apply because no
//!    amount of waiting can rescue a stale snapshot. `BEGIN IMMEDIATE` takes the
//!    write lock up front, where the busy handler *does* apply.
//! 2. **Writes retry on busy** ([`retry_write`]) with exponential backoff and
//!    jitter, bounded by [`WritePolicy`]. Only an exhausted budget surfaces as
//!    [`Error::DbBusy`], which ingestion answers with `429 Retry-After` so the
//!    SDK re-sends the event instead of dropping it.
//!
//! Bulk deletes additionally run in [`WritePolicy::delete_batch`]-sized chunks
//! (see the retention sweep in [`super::event`]) so one sweep cannot hold the
//! write lock for seconds.

use sqlx::{Sqlite, Transaction};

use super::Db;
use crate::error::Result;

pub(crate) use crate::adapters::retry::retry_write;

/// Begin a transaction that intends to write, taking SQLite's write lock up
/// front (`BEGIN IMMEDIATE`) instead of deferring it to the first write.
///
/// Use this for every read-then-write transaction; a deferred `BEGIN` would fail
/// with `SQLITE_BUSY` the moment a concurrent writer commits between the read
/// and the write (see the module docs). Read-only work needs no transaction and
/// keeps using the pool directly.
pub(crate) async fn begin_write(db: &Db) -> Result<Transaction<'static, Sqlite>> {
    Ok(db.begin_with("BEGIN IMMEDIATE").await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn begin_write_takes_the_write_lock_up_front() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .expect("create table");

        // The statement is accepted by SQLite (a wrong keyword would error here)
        // and the transaction behaves like any other.
        let mut tx = begin_write(&pool).await.expect("begin immediate");
        sqlx::query("INSERT INTO t (id) VALUES (1)")
            .execute(&mut *tx)
            .await
            .expect("insert");
        tx.commit().await.expect("commit");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 1);
    }
}
