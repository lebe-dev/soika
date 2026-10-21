# Database architecture

soika persists everything in one relational database: **SQLite** (the default,
a single file) or **PostgreSQL 15+**. Both backends hold the same data model and
implement the same repository ports; which one runs is decided at startup by
the scheme of `DATABASE_URL`.

## Layers

```text
 api / ingest / auth / scheduler / notify
                │  Arc<dyn …Repository>  (AppState)
                ▼
        src/ports/*.rs          repository traits, domain types only
                ▲
     ┌──────────┴───────────┐
     │                      │
 src/adapters/sqlite   src/adapters/postgres     one adapter per port and backend
     │                      │
     └──────────┬───────────┘
                ▼
           src/db.rs            backend detection, pools, migrators
```

- **Ports** (`src/ports/`) are `async_trait` traits returning domain types and
  `soika::Result`. Nothing above them knows which database is in use.
- **Adapters** implement every port twice: `Sqlite<Name>Repository` in
  `src/adapters/sqlite/` and `Pg<Name>Repository` in `src/adapters/postgres/`.
  Dialect and column-type concerns live only there (`convert.rs` in each).
- **Shared adapter helpers**: `src/adapters/retry.rs` (bounded write retries)
  and `src/adapters/short_id.rs` (public short-id generator).
- **`src/db.rs`** owns `DbBackend::from_url`, the `DbPool` enum,
  `connect_pool` (SQLite), `connect_pg_pool` (PostgreSQL), `connect_database`
  (either), and the embedded migrators `MIGRATOR` / `PG_MIGRATOR`.
- **Wiring**: `soika::build_state(pool, config)` accepts anything convertible
  into `DbPool` and builds the repository set of that backend
  (`sqlite_repositories` / `postgres_repositories` in `src/lib.rs`).

Queries are runtime-checked (`sqlx::query*` + `FromRow` rows), so the crate
builds without a live database or an offline query cache.

## Backend selection

| `DATABASE_URL` | Backend | Notes |
| --- | --- | --- |
| `sqlite://data/soika.db` | SQLite | File created if missing; WAL mode. |
| `sqlite::memory:` | SQLite | In-memory; used by tests. |
| `postgres://user:pass@host:5432/db`, `postgresql://…` | PostgreSQL | Database must exist; `?sslmode=require` for TLS. |

Any other scheme fails startup with a validation error. Both drivers are always
compiled in and use the rustls/ring TLS stack.

Startup (`src/main.rs`): detect backend → open the pool → run that backend's
migrations → (SQLite only) backfill missing short ids → build `AppState`.

## Data model

All ids are UUIDs except `sessions.id` and `invites.token` (opaque strings) and
the singleton `service_settings.id`. Every foreign key cascades on delete,
except `created_by` columns, which become `NULL` when the user is deleted.

| Table | Purpose | Key relations |
| --- | --- | --- |
| `users` | Accounts: email (unique), password hash (empty for OIDC), `instance_role` (`owner`/`manager`/`member`), `status`, `locale`, `auth_provider`. | — |
| `sessions` | Server-side login sessions with expiry. | → `users` |
| `teams` | Teams (unique name). | — |
| `team_members` | Team membership with `role` (`admin`/`contributor`); the only path from users to projects. | → `teams`, → `users` |
| `projects` | Projects: `short_id`, unique `slug` and `dsn_public_key`, retention settings, mute flag, webhook URL. | → `teams` |
| `project_favorites` | Per-user favorite projects. | → `users`, → `projects` |
| `issues` | Grouped errors, unique per `(project_id, fingerprint)`; status, mute bookkeeping, `first_seen`/`last_seen`, `event_count`. | → `projects` |
| `events` | Individual occurrences; full event JSON in `payload`; `project_id` denormalized for retention. | → `issues`, → `projects` |
| `invites` | Team invites with role, optional target email, expiry, acceptance time. | → `teams`, → `users` (`created_by`) |
| `tag_mute_rules`, `tag_mute_rule_tags` | Project-level "mute by tags" rules and their key/value pairs. | → `projects`; tags → rules |
| `passkeys` | WebAuthn credentials (unique `credential_id`). | → `users` |
| `service_settings` | Single row (`id = 1`): signup toggle, organization name. | — |

Important indexes: `issues (project_id, fingerprint)` unique (grouping);
`issues (project_id, status, last_seen)` (issue lists);
`events (project_id, received_at)` and `events (issue_id, received_at)`
(retention, issue detail); `issues (status, muted_until)` (mute expiry sweep).

### Column types per backend

| Domain type | SQLite | PostgreSQL |
| --- | --- | --- |
| `Id` (UUID) | `TEXT`, lowercase hyphenated | `UUID` |
| `Timestamp` (UTC) | `TEXT`, RFC 3339 (sorts lexically) | `TIMESTAMPTZ`, microsecond precision |
| `bool` | `INTEGER` 0/1 | `BOOLEAN` |
| `i64` counters, retention | `INTEGER` | `BIGINT` |
| Enums (roles, statuses) | `TEXT` | `TEXT` |
| Event payload | `TEXT` (JSON) | `TEXT` (JSON) |

Event payloads stay `TEXT` on PostgreSQL: `JSONB` rejects the `\u0000` escapes
some SDK payloads carry, and the payload is never queried by field.

PostgreSQL adapters truncate timestamps to microseconds before writing them
(`ts` / `now` in `src/adapters/postgres/convert.rs`), so a value returned by a
repository equals what a later read returns.

## Migrations

| Backend | Directory | Migrator |
| --- | --- | --- |
| SQLite | `migrations/sqlite/` | `soika::MIGRATOR` |
| PostgreSQL | `migrations/postgres/` | `soika::PG_MIGRATOR` |

- Migrations are embedded at compile time and run automatically at startup;
  sqlx records them in `_sqlx_migrations`.
- The histories are numbered independently. SQLite carries its full history
  (including table rebuilds); PostgreSQL starts from one consolidated schema
  equal to the SQLite end state.
- A schema change adds a new numbered file to **both** directories. Never edit
  an applied migration — sqlx refuses to start on a checksum mismatch.

## Writes and concurrency

### SQLite

SQLite allows one writer per database file.

- Every write transaction starts with `BEGIN IMMEDIATE` (`begin_write` in
  `src/adapters/sqlite/write.rs`), taking the write lock up front so
  `busy_timeout` applies. A deferred `BEGIN` would fail instantly with
  `SQLITE_BUSY` when it tries to upgrade.
- Read-then-write paths (issue upsert, fingerprint override) are safe because
  the whole transaction holds the single write lock.
- The pool runs in WAL mode with `foreign_keys = ON`, configured only in
  `connect_pool`.

### PostgreSQL

Writers run concurrently under `READ COMMITTED`, so read-then-write paths use
the unique index and row locks instead of a database-wide lock:

- **Issue upsert** (one per ingested event): `INSERT … ON CONFLICT
  (project_id, fingerprint) DO NOTHING RETURNING …` decides "new issue"
  atomically; otherwise the existing row is locked with `SELECT … FOR UPDATE`,
  checked for regression and bumped. An attempt that loses a race to a
  concurrent delete is re-run.
- **Fingerprint override** locks the target and the colliding row
  (`FOR UPDATE`). A rename that collides with a concurrent insert is re-run and
  then takes the merge branch.
- Idempotent inserts use `ON CONFLICT DO NOTHING` / `DO UPDATE`.

### Shared rules

- `retry_write(&policy, "operation", || …)` (`src/adapters/retry.rs`) re-runs
  one self-contained transaction or statement on a transient error, with
  exponential backoff and jitter (`DB_WRITE_*`).
- Transient errors are classified by `Error::is_busy`: SQLite
  `SQLITE_BUSY`/`SQLITE_LOCKED` (including extended codes) and PostgreSQL
  SQLSTATE `40001` (serialization failure), `40P01` (deadlock), `55P03` (lock
  not available).
- A spent retry budget becomes `Error::DbBusy`: ingestion answers
  `429 Retry-After: DB_BUSY_RETRY_AFTER_SECS` (SDKs re-send the event), the
  JSON API answers `503`.
- Retention deletes run in `DB_DELETE_BATCH` chunks on both backends, so a
  large sweep never holds a lock or a transaction for long.
- UNIQUE violations map to `Error::Conflict` (`409`).

## Configuration

| Variable | SQLite | PostgreSQL |
| --- | --- | --- |
| `DATABASE_URL` | ✓ | ✓ |
| `DB_MAX_CONNECTIONS`, `DB_ACQUIRE_TIMEOUT_MS` | ✓ | ✓ |
| `DB_WRITE_MAX_RETRIES`, `DB_WRITE_RETRY_BASE_MS`, `DB_WRITE_RETRY_MAX_MS` | ✓ | ✓ |
| `DB_DELETE_BATCH`, `DB_BUSY_RETRY_AFTER_SECS` | ✓ | ✓ |
| `DB_BUSY_TIMEOUT_MS`, `DB_SYNCHRONOUS` | ✓ | ignored |

## Testing

- `tests/common` gives every integration test a fresh, migrated database:
  in-memory SQLite by default, PostgreSQL when `SOIKA_TEST_DB=postgres`.
- On PostgreSQL, testcontainers starts one shared, reusable
  `postgres:15-alpine` container (`soika-test-postgres`) per run and each test
  creates its own database in it. `just test-postgres` removes the container
  afterwards, pass or fail.
- `tests/repositories/` is the backend-neutral contract suite for every port.
  It seeds data through the ports only (no SQL), so the same assertions run on
  both backends.
- `tests/postgres_backend.rs` checks PostgreSQL-only behaviour on a live
  server (deadlock classification, re-running migrations); it skips on SQLite.
- SQLite-only: `tests/sqlite_locking.rs` (write-lock contention),
  `tests/migration_0013.rs` (data migration), and the inline tests in
  `src/adapters/sqlite/`.

```sh
just test-backend [name]    # SQLite
just test-postgres [name]   # PostgreSQL 15 (Docker)
just test-all               # SQLite + frontend + PostgreSQL
```

## Adding a table or repository

1. Add a numbered migration to `migrations/sqlite/` and `migrations/postgres/`.
2. Define or extend the port in `src/ports/`.
3. Implement it in `src/adapters/sqlite/` and `src/adapters/postgres/`; route
   every write through `retry_write`, use `begin_write` for SQLite write
   transactions, and lock rows / use `ON CONFLICT` for PostgreSQL
   read-then-write paths.
4. Add the `AppState` field and wire it in both `sqlite_repositories` and
   `postgres_repositories`.
5. Cover it in `tests/repositories/` and run both `just test-backend` and
   `just test-postgres`.

## Limitations

- There is no tool to copy an existing SQLite database into PostgreSQL;
  switching backends starts from an empty instance.
- Run a single replica on either backend: the retention scheduler, ingestion
  rate limiter and login lockout are per process.
- `ORDER BY name` follows the database collation, so name ordering on
  PostgreSQL can differ from SQLite's byte order (for example in mixed case).
