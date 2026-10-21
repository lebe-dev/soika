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
- `src/adapters/` implements those traits; SQLite repositories are in
  `src/adapters/sqlite/`.
- `src/api/` contains the session-authenticated JSON API.
- `src/ingest/` handles Sentry-compatible event ingestion.
- `src/auth/`, `src/grouping/`, `src/mail.rs`, `src/notify.rs`, and
  `src/scheduler.rs` contain feature logic.
- `frontend/` is the SvelteKit SPA. Its static build is written to `web/dist`
  and embedded by `src/web.rs`.
- `migrations/` contains the SQLite schema migrations.

`AppState` in `src/state.rs` is wired in `soika::build_state`. When adding a
repository-backed feature, add its port, adapter, state field, and wiring.
`src/router.rs` is the single source of route wiring. Keep cookie-authenticated
API CORS separate from the permissive DSN-authenticated ingestion routes.

## Database changes

Add a new numbered SQL file under `migrations/`; never edit a migration that
has already been applied. `soika::MIGRATOR` runs migrations at startup.

SQLite has one writer. Every adapter write transaction must use
`begin_write(&db)` from `src/adapters/sqlite/write.rs`, which starts
`BEGIN IMMEDIATE`; do not use deferred `db.begin()`. Wrap one transaction or
one statement in `retry_write(&policy, "operation", || ...)`. Keep retries
self-contained so a retry cannot repeat already-committed work. Bulk deletes
must use bounded batches so retention does not hold the write lock for too
long. See existing issue and event repositories for examples.

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

Common checks:

```sh
just test-backend              # all backend tests
just test-backend <name>       # focused backend test
just test-frontend             # all frontend tests
just lint                      # backend lint (also formats)
just lint-frontend             # Prettier and Svelte checks
```

## Git workflow

Commit each completed task in English using Conventional Commits. The user
pushes changes; do not push from the agent workflow. `AGENTS.md` is local
guidance and must not be added to Git.
