ALTER TABLE projects ADD COLUMN orphaned_at TIMESTAMPTZ;
ALTER TABLE teams ADD COLUMN orphaned_at TIMESTAMPTZ;
CREATE TABLE team_member_grants(team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE, email TEXT NOT NULL, role TEXT NOT NULL CHECK(role IN ('admin', 'contributor')), PRIMARY KEY(team_id, email));
CREATE TABLE team_group_grants(team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE, group_path TEXT NOT NULL, role TEXT NOT NULL CHECK(role IN ('admin', 'contributor')), PRIMARY KEY(team_id, group_path));
CREATE TABLE user_sso_groups(user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE, group_path TEXT NOT NULL, seen_at TIMESTAMPTZ NOT NULL, PRIMARY KEY(user_id, group_path));
