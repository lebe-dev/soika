CREATE TABLE notification_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    config TEXT NOT NULL
);
INSERT INTO notification_settings (id, config) VALUES (1, '{}');
CREATE TABLE notification_alerts (
    issue_id TEXT PRIMARY KEY,
    payload TEXT NOT NULL
);
