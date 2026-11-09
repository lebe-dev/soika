ALTER TABLE notification_settings ADD COLUMN severity_mode TEXT NOT NULL DEFAULT 'fixed' CHECK (severity_mode IN ('fixed', 'event'));
ALTER TABLE projects ADD COLUMN alertmanager_severity_mode TEXT NOT NULL DEFAULT '' CHECK (alertmanager_severity_mode IN ('', 'fixed', 'event'));
UPDATE projects SET alertmanager_severity_mode = 'fixed' WHERE alertmanager_severity <> '';
ALTER TABLE issues ADD COLUMN max_event_severity INTEGER NOT NULL DEFAULT 0 CHECK (max_event_severity BETWEEN 0 AND 3);
UPDATE issues SET max_event_severity = CASE LOWER(TRIM(COALESCE(level, '')))
    WHEN 'fatal' THEN 3 WHEN 'error' THEN 3 WHEN 'warning' THEN 2 WHEN 'info' THEN 1 WHEN 'debug' THEN 1 ELSE 0 END;
