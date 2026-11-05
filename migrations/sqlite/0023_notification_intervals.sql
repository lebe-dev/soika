ALTER TABLE notification_settings ADD COLUMN refresh_interval_seconds BIGINT NOT NULL DEFAULT 60
    CHECK (refresh_interval_seconds BETWEEN 10 AND 86400);
ALTER TABLE projects ADD COLUMN alertmanager_interval_seconds BIGINT NOT NULL DEFAULT 0
    CHECK (alertmanager_interval_seconds = 0 OR alertmanager_interval_seconds BETWEEN 10 AND 86400);
