# Developer guide

This guide covers the minimum needed to work on soika. The project is a Rust
workspace with a SvelteKit frontend; `just` is the task runner and yarn manages
frontend dependencies.

## Local setup

Install the Rust toolchain from `rust-toolchain.toml`, `just`, Node.js, and yarn.
Copy `.env.example` to `.env` and set a local `SECRET_KEY`. The Justfile loads
`.env` automatically.

Useful commands:

```sh
just run             # backend on the configured BIND_ADDR
just run-frontend    # frontend dev server on port 4200
just init-env        # seed the local database with demo data
just build           # build the Rust workspace
just dist            # build frontend assets embedded in the binary
```

The backend defaults to `sqlite://soika.db`. On first run, open `/setup` to
create the initial Owner account. For the frontend-only dev server, configure
its API proxy in `frontend/vite.config.ts` as needed for your local backend.
Configuration variables and their defaults are documented in `.env.example`.

## Project structure

- `src/domain/` contains data types and rules without IO.
- `src/ports/` defines repository and service traits.
- `src/adapters/` implements those traits; repositories exist once per
  database backend, in `src/adapters/sqlite/` and `src/adapters/postgres/`.
- `src/db.rs` picks the backend from `DATABASE_URL`, opens the pool and holds
  the migrators.
- `src/api/` contains the session-authenticated JSON API.
- `src/ingest/` handles Sentry-compatible event ingestion.
- `src/auth/`, `src/grouping/`, `src/mail.rs`, `src/notify.rs`, and
  `src/scheduler.rs` contain feature logic.
- `frontend/` is the SvelteKit SPA. Its static build is written to `web/dist`
  and embedded by `src/web.rs`.
- `migrations/sqlite/` and `migrations/postgres/` contain the schema
  migrations of each backend.

`AppState` in `src/state.rs` is wired in `soika::build_state`. When adding a
repository-backed feature, add its port, both adapters (SQLite and PostgreSQL),
the state field, the wiring for both backends, and contract tests in
`tests/repositories/`.
`src/router.rs` is the single source of route wiring. Keep cookie-authenticated
API CORS separate from the permissive DSN-authenticated ingestion routes.

## Database changes

The backends, data model, migrations and write-concurrency rules are described
in [Database architecture](docs/DATABASE.md).

Every schema change is a new numbered SQL file in **both** `migrations/sqlite/`
and `migrations/postgres/`; never edit a migration that has already been
applied. The histories are numbered independently (PostgreSQL starts from one
consolidated schema). `soika::MIGRATOR` (SQLite) and `soika::PG_MIGRATOR`
(PostgreSQL) run at startup. PostgreSQL uses native types: `UUID` ids,
`TIMESTAMPTZ` timestamps, `BOOLEAN` flags, `BIGINT` counters.

SQLite has one writer. Every adapter write transaction must use
`begin_write(&db)` from `src/adapters/sqlite/write.rs`, which starts
`BEGIN IMMEDIATE`; do not use deferred `db.begin()`. Wrap one transaction or
one statement in `retry_write(&policy, "operation", || ...)`. Keep retries
self-contained so a retry cannot repeat already-committed work. Bulk deletes
must use bounded batches so retention does not hold the write lock for too
long. See existing issue and event repositories for examples.

PostgreSQL runs writers concurrently under `READ COMMITTED`, so a
read-then-write must not rely on a database-wide lock: decide "insert or
update" with `INSERT … ON CONFLICT`, lock rows you are about to change with
`SELECT … FOR UPDATE`, and keep using `retry_write` (shared from
`src/adapters/retry.rs`) — deadlocks and serialization failures count as busy.
`TIMESTAMPTZ` keeps microseconds, so PostgreSQL adapters truncate timestamps
before writing them (`ts` / `now` in `src/adapters/postgres/convert.rs`).

## Change conventions

- Keep dependencies pointed inward: domain rules depend on ports, and adapters
  implement ports.
- Prefer early returns and named exports. The library uses `soika::Error` /
  `soika::Result`; the binary uses `anyhow`.
- Update the relevant document when user-facing or operator behavior changes.
- After frontend changes, run `just dist` before building or releasing the
  binary so it embeds the current SPA.
- Before pushing, run `just lint` and `just lint-frontend` as relevant. The
  repository may not be consistently rustfmt-clean; `just lint` is the backend
  gate.

Integration tests build their database through `tests/common`: an in-memory
SQLite database by default, or PostgreSQL when `SOIKA_TEST_DB=postgres` (what
`just test-postgres` sets). The PostgreSQL run starts one shared
`postgres:15-alpine` container through testcontainers and creates a fresh
database per test. Repository behaviour is covered once, backend-neutrally,
in `tests/repositories/`.

Common checks:

```sh
just test-backend              # all backend tests (SQLite)
just test-backend <name>       # focused backend test
just test-postgres [name]      # backend tests on PostgreSQL 15 (Docker)
just test-all                  # SQLite + frontend + PostgreSQL
just test-frontend             # all frontend tests
just lint                      # backend lint (also formats)
just lint-frontend             # Prettier and Svelte checks
```

