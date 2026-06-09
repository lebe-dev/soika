CREATE TABLE project_favorites (
    user_id    TEXT NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, project_id)
);
