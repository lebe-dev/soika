-- soika PostgreSQL schema
--
-- Consolidated equivalent of the SQLite history (migrations/sqlite 0001..0015)
-- using native Postgres types:
--   * ids are UUID; timestamps are TIMESTAMPTZ (microsecond precision, UTC);
--   * flags are BOOLEAN; counters / retention are BIGINT (Rust `i64`);
--   * event payloads stay TEXT holding the JSON as received — JSONB would
--     reject the `\u0000` escapes some SDK payloads carry, and the payload is
--     never queried by field.
--
-- Future schema changes are added as new numbered files here *and* in
-- migrations/sqlite so both backends stay in step.

-- ---------------------------------------------------------------------------
-- Users & sessions
-- ---------------------------------------------------------------------------
CREATE TABLE users (
    id                     UUID PRIMARY KEY,
    email                  TEXT NOT NULL UNIQUE,
    display_name           TEXT NOT NULL,
    password_hash          TEXT NOT NULL,              -- empty for OIDC accounts
    notifications_enabled  BOOLEAN NOT NULL DEFAULT TRUE,
    created_at             TIMESTAMPTZ NOT NULL,
    updated_at             TIMESTAMPTZ NOT NULL,
    auth_provider          TEXT NOT NULL DEFAULT 'local',   -- 'local' | 'oidc'
    status                 TEXT NOT NULL DEFAULT 'active',  -- 'active' | 'pending'
    instance_role          TEXT NOT NULL DEFAULT 'member',  -- 'owner' | 'manager' | 'member'
    locale                 TEXT CHECK (locale IN ('en', 'ru'))
);

CREATE TABLE sessions (
    id          TEXT PRIMARY KEY,           -- opaque session id
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_sessions_user ON sessions (user_id);
CREATE INDEX idx_sessions_expires ON sessions (expires_at);

-- ---------------------------------------------------------------------------
-- Teams & membership (User × Team × TeamRole)
-- ---------------------------------------------------------------------------
CREATE TABLE teams (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE team_members (
    team_id     UUID NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'contributor',  -- 'admin' | 'contributor'
    created_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (team_id, user_id)
);

CREATE INDEX idx_team_members_user ON team_members (user_id);

-- ---------------------------------------------------------------------------
-- Projects
-- ---------------------------------------------------------------------------
CREATE TABLE projects (
    id                UUID PRIMARY KEY,
    short_id          TEXT NOT NULL UNIQUE,            -- public id used in URLs
    team_id           UUID NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    slug              TEXT NOT NULL UNIQUE,
    dsn_public_key    TEXT NOT NULL UNIQUE,            -- DSN auth
    retention_events  BIGINT NOT NULL DEFAULT 1000,
    retention_days    BIGINT NOT NULL DEFAULT 0,       -- 0 disables age-based pruning
    muted             BOOLEAN NOT NULL DEFAULT FALSE,
    webhook_url       TEXT,                            -- NULL = no webhook channel
    created_at        TIMESTAMPTZ NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_projects_team ON projects (team_id);

CREATE TABLE project_favorites (
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, project_id)
);

-- ---------------------------------------------------------------------------
-- Issues (grouped errors)
-- ---------------------------------------------------------------------------
CREATE TABLE issues (
    id                   UUID PRIMARY KEY,
    short_id             TEXT NOT NULL UNIQUE,
    project_id           UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    fingerprint          TEXT NOT NULL,
    title                TEXT NOT NULL,
    culprit              TEXT,
    level                TEXT,
    environment          TEXT,
    release              TEXT,
    status               TEXT NOT NULL DEFAULT 'unresolved', -- 'unresolved' | 'resolved' | 'muted'
    muted_at             TIMESTAMPTZ,
    muted_until          TIMESTAMPTZ,       -- time-based mute expiry
    mute_threshold       BIGINT,            -- event-rate mute: events per window
    mute_window_seconds  BIGINT,
    first_seen           TIMESTAMPTZ NOT NULL,
    last_seen            TIMESTAMPTZ NOT NULL,
    event_count          BIGINT NOT NULL DEFAULT 0,
    created_at           TIMESTAMPTZ NOT NULL,
    updated_at           TIMESTAMPTZ NOT NULL
);

-- Grouping: one issue per (project, fingerprint).
CREATE UNIQUE INDEX idx_issues_project_fingerprint ON issues (project_id, fingerprint);
CREATE INDEX idx_issues_last_seen ON issues (last_seen);
CREATE INDEX idx_issues_project_status ON issues (project_id, status, last_seen);
CREATE INDEX idx_issues_project_environment ON issues (project_id, environment);
CREATE INDEX idx_issues_project_release ON issues (project_id, release);
CREATE INDEX idx_issues_muted_until ON issues (status, muted_until);

-- ---------------------------------------------------------------------------
-- Events (individual occurrences)
-- ---------------------------------------------------------------------------
CREATE TABLE events (
    id           UUID PRIMARY KEY,
    event_id     TEXT NOT NULL,             -- Sentry event_id (hex32)
    issue_id     UUID NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    project_id   UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    payload      TEXT NOT NULL,             -- full event JSON as received
    received_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_events_project_received ON events (project_id, received_at);
CREATE INDEX idx_events_issue_received ON events (issue_id, received_at);

-- ---------------------------------------------------------------------------
-- Team invites
-- ---------------------------------------------------------------------------
CREATE TABLE invites (
    token       TEXT PRIMARY KEY,
    team_id     UUID NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'contributor',  -- 'admin' | 'contributor'
    email       TEXT,
    created_by  UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ
);

CREATE INDEX idx_invites_team ON invites (team_id);
CREATE INDEX idx_invites_email ON invites (email);

-- ---------------------------------------------------------------------------
-- Service settings — single-row table, id always = 1
-- ---------------------------------------------------------------------------
CREATE TABLE service_settings (
    id            SMALLINT PRIMARY KEY CHECK (id = 1),
    allow_signup  BOOLEAN NOT NULL DEFAULT FALSE,
    org_name      TEXT NOT NULL DEFAULT 'soika',
    updated_at    TIMESTAMPTZ NOT NULL
);

INSERT INTO service_settings (id, allow_signup, org_name, updated_at)
VALUES (1, FALSE, 'soika', '1970-01-01T00:00:00Z');

-- ---------------------------------------------------------------------------
-- Project-level "mute by tags" rules
-- ---------------------------------------------------------------------------
CREATE TABLE tag_mute_rules (
    id         UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name       TEXT,
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_tag_mute_rules_project ON tag_mute_rules (project_id);

CREATE TABLE tag_mute_rule_tags (
    rule_id   UUID NOT NULL REFERENCES tag_mute_rules (id) ON DELETE CASCADE,
    tag_key   TEXT NOT NULL,
    tag_value TEXT NOT NULL,
    PRIMARY KEY (rule_id, tag_key)
);

-- ---------------------------------------------------------------------------
-- Passkeys (WebAuthn credentials)
-- ---------------------------------------------------------------------------
CREATE TABLE passkeys (
    id             UUID PRIMARY KEY,
    user_id        UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    credential_id  TEXT NOT NULL UNIQUE,      -- base64url, no padding
    name           TEXT NOT NULL,
    credential     TEXT NOT NULL,             -- opaque serialized credential
    created_at     TIMESTAMPTZ NOT NULL,
    last_used_at   TIMESTAMPTZ
);

CREATE INDEX idx_passkeys_user ON passkeys (user_id);
