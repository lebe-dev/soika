-- soika initial schema (MVP §4 Data Model)
--
-- Design notes:
--   * SQL is kept ANSI-friendly so a PostgreSQL backend can be added later
--     (MVP §2.2). SQLite-specific bits are isolated and commented.
--   * IDs are stored as TEXT (UUID v4 string form) — portable across SQLite and
--     Postgres. Timestamps are TEXT in RFC3339/ISO-8601 form (UTC).
--   * Booleans are stored as INTEGER 0/1 (SQLite has no native BOOL; this is also
--     valid under Postgres with a cast, or trivially migratable).
--   * JSON payloads stored as TEXT.

-- ---------------------------------------------------------------------------
-- Users
-- ---------------------------------------------------------------------------
CREATE TABLE users (
    id                     TEXT PRIMARY KEY,
    email                  TEXT NOT NULL UNIQUE,
    display_name           TEXT NOT NULL,
    password_hash          TEXT NOT NULL,
    is_admin               INTEGER NOT NULL DEFAULT 0,   -- instance-wide admin (§11)
    notifications_enabled  INTEGER NOT NULL DEFAULT 1,   -- profile opt-out (§10.3)
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_users_email ON users (email);

-- ---------------------------------------------------------------------------
-- Sessions (server-side, opaque token — §10.1)
-- ---------------------------------------------------------------------------
CREATE TABLE sessions (
    id          TEXT PRIMARY KEY,          -- opaque session id (also the cookie value seed)
    user_id     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE INDEX idx_sessions_user ON sessions (user_id);
CREATE INDEX idx_sessions_expires ON sessions (expires_at);

-- ---------------------------------------------------------------------------
-- Teams (§4.1)
-- ---------------------------------------------------------------------------
CREATE TABLE teams (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Team membership (User × Team)
CREATE TABLE team_members (
    team_id     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (team_id, user_id),
    FOREIGN KEY (team_id) REFERENCES teams (id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE INDEX idx_team_members_user ON team_members (user_id);

-- ---------------------------------------------------------------------------
-- Projects (§8.2)
-- ---------------------------------------------------------------------------
CREATE TABLE projects (
    id                TEXT PRIMARY KEY,
    team_id           TEXT NOT NULL,
    name              TEXT NOT NULL,
    slug              TEXT NOT NULL UNIQUE,
    dsn_public_key    TEXT NOT NULL UNIQUE,           -- DSN auth (§5.1)
    retention_events  INTEGER NOT NULL DEFAULT 1000,  -- overrides DEFAULT_EVENTS_RETENTION
    muted             INTEGER NOT NULL DEFAULT 0,      -- project-level mute (§8.2)
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    FOREIGN KEY (team_id) REFERENCES teams (id) ON DELETE CASCADE
);

CREATE INDEX idx_projects_team ON projects (team_id);
CREATE UNIQUE INDEX idx_projects_dsn ON projects (dsn_public_key);

-- ---------------------------------------------------------------------------
-- Memberships (User × Project × Role — §10.2)
-- ---------------------------------------------------------------------------
CREATE TABLE memberships (
    project_id  TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'member',   -- 'admin' | 'member'
    created_at  TEXT NOT NULL,
    PRIMARY KEY (project_id, user_id),
    FOREIGN KEY (project_id) REFERENCES projects (id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE INDEX idx_memberships_user ON memberships (user_id);

-- ---------------------------------------------------------------------------
-- Issues (grouped errors — §7, §8.1)
-- ---------------------------------------------------------------------------
CREATE TABLE issues (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NOT NULL,
    fingerprint  TEXT NOT NULL,
    title        TEXT NOT NULL,
    culprit      TEXT,
    level        TEXT,                            -- error/warning/info/...
    status       TEXT NOT NULL DEFAULT 'unresolved', -- 'unresolved' | 'resolved' | 'muted'
    first_seen   TEXT NOT NULL,
    last_seen    TEXT NOT NULL,
    event_count  INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects (id) ON DELETE CASCADE
);

-- Grouping: one issue per (project, fingerprint) (§7).
CREATE UNIQUE INDEX idx_issues_project_fingerprint ON issues (project_id, fingerprint);
-- Issue list ordering / "most recently seen".
CREATE INDEX idx_issues_last_seen ON issues (last_seen);
CREATE INDEX idx_issues_project_status ON issues (project_id, status, last_seen);

-- ---------------------------------------------------------------------------
-- Events (individual occurrences — §5, §13)
-- ---------------------------------------------------------------------------
CREATE TABLE events (
    id           TEXT PRIMARY KEY,          -- internal id
    event_id     TEXT NOT NULL,             -- Sentry event_id (hex32)
    issue_id     TEXT NOT NULL,
    project_id   TEXT NOT NULL,             -- denormalized for retention queries
    payload      TEXT NOT NULL,             -- full event JSON as received
    received_at  TEXT NOT NULL,
    FOREIGN KEY (issue_id) REFERENCES issues (id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES projects (id) ON DELETE CASCADE
);

-- Ingestion / retention: events per project ordered by recency (§13 prune).
CREATE INDEX idx_events_project_received ON events (project_id, received_at);
-- Issue detail: events for an issue, newest first.
CREATE INDEX idx_events_issue_received ON events (issue_id, received_at);

-- ---------------------------------------------------------------------------
-- Invites (§9)
-- ---------------------------------------------------------------------------
CREATE TABLE invites (
    token       TEXT PRIMARY KEY,           -- signed, opaque token
    project_id  TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'member',
    email       TEXT,                        -- optional target email
    created_by  TEXT,                        -- user id of inviting admin
    created_at  TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    accepted_at TEXT,
    FOREIGN KEY (project_id) REFERENCES projects (id) ON DELETE CASCADE,
    FOREIGN KEY (created_by) REFERENCES users (id) ON DELETE SET NULL
);

CREATE INDEX idx_invites_project ON invites (project_id);
CREATE INDEX idx_invites_email ON invites (email);

-- ---------------------------------------------------------------------------
-- Service settings (§14) — single-row table, id always = 1
-- ---------------------------------------------------------------------------
CREATE TABLE service_settings (
    id            INTEGER PRIMARY KEY,        -- always 1 (singleton)
    allow_signup  INTEGER NOT NULL DEFAULT 0,
    org_name      TEXT NOT NULL DEFAULT 'soika',
    updated_at    TEXT NOT NULL
);

INSERT INTO service_settings (id, allow_signup, org_name, updated_at)
VALUES (1, 0, 'soika', '1970-01-01T00:00:00Z');
