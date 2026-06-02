-- Story 7.1: age-based event retention (`retention_days`).
--
-- Adds a per-project age-based retention window, complementing the existing
-- count-based `retention_events`. Events older than `retention_days` are pruned
-- by the cron sweep (src/scheduler/mod.rs).
--
--   * 0 means disabled / no age-based pruning (existing rows backfill to 0).
--   * SQLite permits ADD COLUMN with a NOT NULL constant DEFAULT; this is
--     ANSI-friendly and portable to Postgres.
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR.

ALTER TABLE projects ADD COLUMN retention_days INTEGER NOT NULL DEFAULT 0;
