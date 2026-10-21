-- Story 4.4 / 5.2: first-class environment + release on issues.
--
-- Promotes the Sentry top-level `environment` and `release` fields to filterable
-- columns on issues (level already exists). Existing rows back-fill to NULL,
-- matching the `Option<String>` domain fields.
--
-- Design notes (mirroring 0001_init.sql):
--   * SQL kept ANSI-friendly; SQLite ALTER TABLE adds one column per statement.
--   * No NOT NULL / no DEFAULT so existing rows are NULL.
--   * `release` is safe as a bare SQLite identifier (mirrors bare `level`/`status`
--     columns); a future Postgres port can keep it unquoted (non-reserved there).
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

ALTER TABLE issues ADD COLUMN environment TEXT;
ALTER TABLE issues ADD COLUMN release TEXT;

-- Optional filter indexes (mirror idx_issues_project_status).
CREATE INDEX idx_issues_project_environment ON issues (project_id, environment);
CREATE INDEX idx_issues_project_release ON issues (project_id, release);
