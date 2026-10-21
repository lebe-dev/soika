-- RBAC phase 1: per-team role on team membership.
--
-- Adds a role to each `team_members` row. Under the new model team membership is
-- the sole grant of project access (Variant A): `admin` can manage the team's
-- composition/roles, its projects and project settings; `contributor` has
-- read/triage access to the team's projects only.
--
-- Design notes (mirroring 0001_init.sql / 0002_project_retention_days.sql):
--   * NOT NULL with DEFAULT 'contributor' so existing rows back-fill to the
--     least-privileged team role. Migration 0013 then promotes the relevant
--     rows to 'admin' from the dropped per-project memberships.
--   * Values: 'admin' | 'contributor'. SQL kept ANSI-friendly; portable to
--     Postgres. SQLite permits ADD COLUMN with a NOT NULL constant DEFAULT.
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

ALTER TABLE team_members ADD COLUMN role TEXT NOT NULL DEFAULT 'contributor';
