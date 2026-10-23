-- Empty until an instance manager explicitly configures defaults.
-- Only future user creation reads this table; existing memberships are untouched.
CREATE TABLE default_teams (
    team_id TEXT NOT NULL PRIMARY KEY REFERENCES teams (id) ON DELETE CASCADE
);
