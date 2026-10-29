ALTER TABLE projects DROP CONSTRAINT projects_team_id_fkey;
ALTER TABLE projects ADD CONSTRAINT projects_team_id_fkey FOREIGN KEY (team_id) REFERENCES teams(id) ON DELETE RESTRICT;
