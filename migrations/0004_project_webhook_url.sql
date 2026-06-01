-- Story 6.2: per-project webhook notification channel (`webhook_url`).
--
-- Adds an optional per-project webhook URL. When set, new-issue and regression
-- notifications are also delivered as an HTTP POST JSON payload to this URL,
-- alongside the existing email channel and under the SAME suppression rules
-- (project mute / issue mute). See src/notify/mod.rs.
--
-- Design notes (mirroring 0001_init.sql):
--   * No NOT NULL / no DEFAULT so existing rows back-fill to NULL, matching the
--     `Option<String>` domain field.
--   * SQL kept ANSI-friendly; SQLite ALTER TABLE adds one column per statement,
--     and this is portable to Postgres (MVP §2.2).
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

ALTER TABLE projects ADD COLUMN webhook_url TEXT;
