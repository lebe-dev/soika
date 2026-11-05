ALTER TABLE projects ADD COLUMN alertmanager_severity TEXT NOT NULL DEFAULT ''
    CHECK (alertmanager_severity IN ('', 'critical', 'warning', 'info'));
