-- no-transaction
-- Foreign keys must be disabled outside a transaction while replacing a parent.
PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;
CREATE TABLE projects_new (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    dsn_public_key TEXT NOT NULL UNIQUE,
    retention_events INTEGER NOT NULL DEFAULT 1000,
    muted INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    retention_days INTEGER NOT NULL DEFAULT 0,
    webhook_url TEXT,
    short_id TEXT
);
INSERT INTO projects_new SELECT id, team_id, name, slug, dsn_public_key, retention_events, muted, created_at, updated_at, retention_days, webhook_url, short_id FROM projects;
DROP TABLE projects;
ALTER TABLE projects_new RENAME TO projects;
CREATE INDEX idx_projects_team ON projects(team_id);
CREATE UNIQUE INDEX idx_projects_dsn ON projects(dsn_public_key);
CREATE UNIQUE INDEX idx_projects_short_id ON projects(short_id);
COMMIT;
PRAGMA foreign_keys = ON;
