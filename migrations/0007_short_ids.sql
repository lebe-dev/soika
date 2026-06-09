-- Short public ids for projects and issues.
--
-- The internal primary keys stay UUID (used by foreign keys, the DSN and the
-- ingestion path); `short_id` is a compact, URL-friendly code (6 chars, a-z0-9)
-- used only in the web UI URLs and the SPA-facing API.
--
-- New rows get a random code from the application. Existing rows are left NULL
-- here and backfilled at startup with unique random codes (see
-- `adapters::sqlite::backfill_short_ids`) — SQL cannot generate a *random and
-- guaranteed-unique* value in one pass without risking a UNIQUE-index failure.
-- SQLite treats NULLs as distinct, so the UNIQUE index below tolerates the
-- transient NULLs until the backfill runs.

ALTER TABLE projects ADD COLUMN short_id TEXT;
ALTER TABLE issues   ADD COLUMN short_id TEXT;

CREATE UNIQUE INDEX idx_projects_short_id ON projects (short_id);
CREATE UNIQUE INDEX idx_issues_short_id   ON issues (short_id);
