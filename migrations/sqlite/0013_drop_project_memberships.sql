-- RBAC phase 1 (RISKIEST): drop per-project memberships; re-scope invites to teams.
--
-- Under Variant A, access to a project comes only from membership in the team
-- that owns the project. The `memberships` (user × project × role) table is
-- removed, but no one may lose access: every existing project membership is
-- folded into team membership of the project's owning team.
--
-- Migration of existing access (order matters):
--   1. Ensure every user who had ANY project membership is a member of each
--      owning team. INSERT OR IGNORE as 'contributor' (the floor) via
--      projects.team_id; existing team_members rows are left untouched here.
--   2. Promote to team 'admin' every (team, user) where the user was
--      memberships.role='admin' in AT LEAST ONE project of that team
--      (Admin -> Team Admin). This runs after step 1 so the rows always exist,
--      and it also upgrades rows that pre-existed in team_members.
--   3. DROP TABLE memberships.
--   4. Rebuild `invites` from project-scoped to team-scoped (project_id ->
--      team_id via projects.team_id; legacy role 'admin' -> 'admin',
--      'member' -> 'contributor'). Orphan invites (project already gone) are
--      dropped — the old FK was ON DELETE CASCADE, so none should exist.
--
-- SQLite cannot DROP COLUMN / re-point a FK portably, so `invites` is rebuilt
-- with the standard idiom (create new table, copy, drop old, rename). Foreign
-- keys are toggled OFF for the rebuild; the pool turns them back ON per
-- connection after the migration.
--
-- Migrations are append-only and auto-applied at startup and in every test via
-- crate::MIGRATOR. Do NOT edit existing migrations.

-- 1. Fold project memberships into team membership (floor role 'contributor').
INSERT OR IGNORE INTO team_members (team_id, user_id, role, created_at)
SELECT p.team_id, m.user_id, 'contributor', m.created_at
FROM memberships m
JOIN projects p ON p.id = m.project_id;

-- 2. Promote to team 'admin' where the user was project-admin in ANY team project.
UPDATE team_members
SET role = 'admin'
WHERE EXISTS (
    SELECT 1
    FROM memberships m
    JOIN projects p ON p.id = m.project_id
    WHERE p.team_id = team_members.team_id
      AND m.user_id = team_members.user_id
      AND m.role = 'admin'
);

-- 3. Drop the per-project memberships table.
DROP TABLE memberships;

-- 4. Re-scope `invites` from project to team via the table-rebuild idiom.
PRAGMA foreign_keys = OFF;

CREATE TABLE invites_new (
    token       TEXT PRIMARY KEY,            -- signed, opaque token
    team_id     TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'contributor', -- 'admin' | 'contributor'
    email       TEXT,                         -- optional target email
    created_by  TEXT,                         -- user id of inviting admin
    created_at  TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    accepted_at TEXT,
    FOREIGN KEY (team_id) REFERENCES teams (id) ON DELETE CASCADE,
    FOREIGN KEY (created_by) REFERENCES users (id) ON DELETE SET NULL
);

INSERT INTO invites_new (
    token, team_id, role, email, created_by, created_at, expires_at, accepted_at
)
SELECT
    i.token,
    p.team_id,
    CASE i.role WHEN 'admin' THEN 'admin' ELSE 'contributor' END,
    i.email,
    i.created_by,
    i.created_at,
    i.expires_at,
    i.accepted_at
FROM invites i
JOIN projects p ON p.id = i.project_id;

DROP TABLE invites;
ALTER TABLE invites_new RENAME TO invites;

CREATE INDEX idx_invites_team ON invites (team_id);
CREATE INDEX idx_invites_email ON invites (email);

PRAGMA foreign_keys = ON;
