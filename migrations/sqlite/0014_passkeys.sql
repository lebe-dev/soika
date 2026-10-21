-- Passkeys (WebAuthn discoverable credentials) bound to a user account.
--
-- One row per registered credential. The credential itself (public key, COSE
-- algorithm, counter, backup flags) is stored as the JSON serialization of the
-- `webauthn_rs::prelude::Passkey` value; `credential_id` is its raw credential
-- id in URL-safe base64 (no padding) so lookups stay ANSI/portable and the
-- uniqueness constraint is enforced by the database.
--
-- The WebAuthn user handle is the `users.id` UUID itself, so a discoverable
-- ("usernameless") assertion resolves straight to an account without a separate
-- handle column.
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

CREATE TABLE passkeys (
    id             TEXT PRIMARY KEY,
    user_id        TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- Raw WebAuthn credential id, URL-safe base64 (no padding). Globally unique:
    -- a credential must never be claimable by two accounts.
    credential_id  TEXT NOT NULL UNIQUE,
    -- Operator-facing label ("MacBook Touch ID"), chosen at registration time.
    name           TEXT NOT NULL,
    -- JSON serialization of the verified credential.
    credential     TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    last_used_at   TEXT
);

CREATE INDEX idx_passkeys_user ON passkeys (user_id);
CREATE UNIQUE INDEX idx_passkeys_credential_id ON passkeys (credential_id);
