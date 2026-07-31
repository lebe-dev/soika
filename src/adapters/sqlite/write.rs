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
use crate::config::WritePolicy;
use crate::error::{Error, Result};

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

/// Run a write, retrying while the database reports a lock conflict.
///
/// `what` names the operation for logs and for the final [`Error::DbBusy`]
/// message. `op` is re-invoked from scratch on each attempt, so it must be
/// self-contained and idempotent as a whole — i.e. one transaction, or one
/// statement. Never wrap several already-committed steps in it: a retry would
/// re-apply the committed ones.
///
/// Non-busy errors propagate on the first attempt; nothing is swallowed.
pub(crate) async fn retry_write<T, F, Fut>(policy: &WritePolicy, what: &str, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 0u32;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) if err.is_busy() => {
                if attempt >= policy.max_retries {
                    tracing::warn!(
                        operation = what,
                        attempts = attempt + 1,
                        error = %err,
                        "database stayed locked; giving up on the write"
                    );
                    return Err(Error::db_busy(what, err));
                }
                let delay = policy.backoff(attempt);
                tracing::debug!(
                    operation = what,
                    attempt = attempt + 1,
                    backoff_ms = delay.as_millis(),
                    "database locked; retrying the write"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    /// Fast policy so the tests don't actually sleep for long.
    fn policy(max_retries: u32) -> WritePolicy {
        WritePolicy {
            max_retries,
            retry_base: Duration::from_millis(1),
            retry_max: Duration::from_millis(2),
            delete_batch: 500,
        }
    }

    fn busy() -> Error {
        Error::db_busy("inner", Error::Db(sqlx::Error::PoolClosed))
    }

    #[tokio::test]
    async fn returns_the_value_without_retrying_when_the_write_succeeds() {
        let calls = AtomicU32::new(0);
        let out = retry_write(&policy(5), "write", || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(7) }
        })
        .await
        .expect("write succeeds");

        assert_eq!(out, 7);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_a_busy_write_until_it_succeeds() {
        let calls = AtomicU32::new(0);
        let out = retry_write(&policy(5), "write", || {
            let attempt = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt < 2 {
                    return Err(busy());
                }
                Ok(attempt)
            }
        })
        .await
        .expect("succeeds on the third attempt");

        assert_eq!(out, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn gives_up_as_db_busy_after_the_budget_is_spent() {
        let calls = AtomicU32::new(0);
        let err = retry_write(&policy(3), "insert event", || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<(), _>(busy()) }
        })
        .await
        .expect_err("stays busy");

        // 1 initial attempt + 3 retries.
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        match err {
            Error::DbBusy { ref what, .. } => assert_eq!(what, "insert event"),
            other => panic!("expected DbBusy, got {other}"),
        }
        assert!(err.is_busy());
    }

    #[tokio::test]
    async fn does_not_retry_a_non_busy_error() {
        let calls = AtomicU32::new(0);
        let err = retry_write(&policy(5), "write", || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<(), _>(Error::Validation("bad".into())) }
        })
        .await
        .expect_err("validation error propagates");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(err, Error::Validation(_)), "got {err}");
    }

    #[tokio::test]
    async fn zero_retries_fails_on_the_first_busy_error() {
        let calls = AtomicU32::new(0);
        let err = retry_write(&policy(0), "write", || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<(), _>(busy()) }
        })
        .await
        .expect_err("retrying is disabled");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(err.is_busy());
    }

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
