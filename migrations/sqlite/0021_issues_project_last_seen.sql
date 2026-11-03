-- The issue list pages one project's issues newest first. Without a status
-- filter no existing index covered (project_id, last_seen), so every page
-- sorted all of the project's issues. The global last_seen index served no
-- query and only cost an extra write on every ingested event.
DROP INDEX IF EXISTS idx_issues_last_seen;
CREATE INDEX idx_issues_project_last_seen ON issues (project_id, last_seen);
