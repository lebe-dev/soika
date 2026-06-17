-- RBAC phase 1: instance-wide role replaces the `users.is_admin` boolean.
--
-- The former boolean `is_admin` becomes a three-valued instance role
-- (`owner | manager | member`). The single built-in admin (and any other
-- previously-admin account) is promoted to `owner`; everyone else defaults to
-- `member`. After the backfill the `is_admin` column is dropped.
--
-- SQLite cannot DROP COLUMN portably here, so we use the standard table-rebuild
-- idiom (create new table, copy, drop old, rename). The rebuilt `users` table is
-- the post-0001/0005/0009 shape minus `is_admin`, plus `instance_role`.
--
-- Design notes (mirroring 0001_init.sql):
--   * `instance_role` is TEXT NOT NULL DEFAULT 'member' — ANSI-friendly,
--     portable to Postgres. Values: 'owner' | 'manager' | 'member'.
--   * Foreign keys are toggled OFF for the rebuild so child rows (sessions,
--     team_members, projects, memberships, invites, project_favorites,
--     tag_mute_rules referencing users) survive the drop/rename; the pool turns
--     foreign_keys back ON per connection after the migration.
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

-- 1. Add the new column and backfill from the legacy boolean.
ALTER TABLE users ADD COLUMN instance_role TEXT NOT NULL DEFAULT 'member';
UPDATE users SET instance_role = 'owner' WHERE is_admin = 1;

-- 2. Drop `is_admin` via the SQLite table-rebuild idiom.
PRAGMA foreign_keys = OFF;

CREATE TABLE users_new (
    id                     TEXT PRIMARY KEY,
    email                  TEXT NOT NULL UNIQUE,
    display_name           TEXT NOT NULL,
    password_hash          TEXT NOT NULL,
    notifications_enabled  INTEGER NOT NULL DEFAULT 1,   -- profile opt-out
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL,
    auth_provider          TEXT NOT NULL DEFAULT 'local', -- 'local' | 'oidc'
    status                 TEXT NOT NULL DEFAULT 'active', -- 'active' | 'pending'
    instance_role          TEXT NOT NULL DEFAULT 'member'  -- 'owner' | 'manager' | 'member'
);

INSERT INTO users_new (
    id, email, display_name, password_hash, notifications_enabled,
    created_at, updated_at, auth_provider, status, instance_role
)
SELECT
    id, email, display_name, password_hash, notifications_enabled,
    created_at, updated_at, auth_provider, status, instance_role
FROM users;

DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE UNIQUE INDEX idx_users_email ON users (email);

PRAGMA foreign_keys = ON;
