-- SQLite requires a table rebuild to change a column default.
CREATE TABLE notification_settings_critical (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    config TEXT NOT NULL,
    refresh_interval_seconds BIGINT NOT NULL DEFAULT 60
        CHECK (refresh_interval_seconds BETWEEN 10 AND 86400),
    default_severity TEXT NOT NULL DEFAULT 'critical'
        CHECK (default_severity IN ('', 'critical', 'warning', 'info'))
);
INSERT INTO notification_settings_critical (id, config, refresh_interval_seconds, default_severity)
SELECT id, config, refresh_interval_seconds,
    CASE WHEN default_severity = '' THEN 'critical' ELSE default_severity END
FROM notification_settings;
DROP TABLE notification_settings;
ALTER TABLE notification_settings_critical RENAME TO notification_settings;
