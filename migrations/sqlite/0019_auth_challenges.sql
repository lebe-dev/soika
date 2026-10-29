-- Opaque handles prevent replay of client-held WebAuthn ceremony state.
CREATE TABLE auth_challenges (
    id TEXT PRIMARY KEY,
    purpose TEXT NOT NULL,
    state TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX auth_challenges_expiry ON auth_challenges(expires_at);

-- Old snapshots lost the original case and cannot safely identify an IdP group.
-- Startup reconciliation removes group-only access until the next verified SSO login.
DELETE FROM user_sso_groups;
