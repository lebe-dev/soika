ALTER TABLE notification_settings ALTER COLUMN default_severity SET DEFAULT 'critical';
UPDATE notification_settings SET default_severity = 'critical' WHERE default_severity = '';
