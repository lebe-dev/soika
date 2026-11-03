-- Records how a team membership was created so that configuration-file
-- reconciliation only removes memberships it owns. Existing rows are treated as
-- configuration-derived, which preserves the previous behaviour.
ALTER TABLE team_members ADD COLUMN source TEXT NOT NULL DEFAULT 'config';
