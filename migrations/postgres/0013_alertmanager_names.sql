ALTER TABLE notification_settings ADD COLUMN alert_names TEXT NOT NULL
    DEFAULT '{"critical":"SoikaIssueCritical","warning":"SoikaIssueWarning","info":"SoikaIssueInfo","project_test":"SoikaIssueCritical"}';
