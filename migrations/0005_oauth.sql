-- Story S2 (OAuth / OIDC): account origin marker.
--
-- OAuth-authenticated users have no local password. SQLite cannot ALTER COLUMN
-- to drop NOT NULL, but we keep `password_hash` NOT NULL and store an empty
-- string sentinel ("") for OIDC accounts. `verify_password` over an empty/invalid
-- PHC always fails, so password login for such accounts is impossible by design.
--
-- This migration only adds an explicit, readable origin flag (`auth_provider`):
-- values 'local' | 'oidc'. It is a forward-looking signal for UI badges,
-- analytics, and disabling password change for OIDC users.
--
-- Design notes (mirroring 0001_init.sql):
--   * NOT NULL with DEFAULT 'local' so existing rows back-fill to local accounts.
--   * SQL kept ANSI-friendly; portable to Postgres (MVP §2.2).
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

ALTER TABLE users ADD COLUMN auth_provider TEXT NOT NULL DEFAULT 'local';
