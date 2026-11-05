ALTER TABLE notification_settings ADD COLUMN default_severity TEXT NOT NULL DEFAULT ''
    CHECK (default_severity IN ('', 'critical', 'warning', 'info'));
