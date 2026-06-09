-- OAuth admin-approval flow: account activation status.
--
-- When OAUTH_REQUIRE_APPROVAL is enabled, a user provisioned on first SSO login
-- lands in `pending` and receives no session until an instance admin approves
-- the account in the admin UI. Password/local accounts (setup, register, invite)
-- are always created `active`, so the flow is opt-in and OIDC-only.
--
-- Design notes (mirroring 0001_init.sql / 0005_oauth.sql):
--   * NOT NULL with DEFAULT 'active' so every existing row back-fills to active —
--     enabling the flag never retroactively locks out already-provisioned users.
--   * Values: 'active' | 'pending'. SQL kept ANSI-friendly; portable to Postgres.
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

ALTER TABLE users ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
