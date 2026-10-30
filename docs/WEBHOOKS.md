# Outgoing webhooks

Set `WEBHOOKS_ENABLED=true`, restart soika, and set **Project settings → Webhook
URL** to an HTTP(S) endpoint accepting [Alertmanager webhook v4 JSON](https://prometheus.io/docs/alerting/latest/configuration/#webhook_config).

Since **0.10.0**, this replaces the previous flat JSON payload. Each new issue
or regression sends one `POST` with `Content-Type: application/json` and one
`firing` alert. Project/issue mute suppresses delivery; SMTP is independent.
Resolving an issue does not send a webhook. This is the outgoing webhook format,
not the Alertmanager `/api/v2/alerts` ingestion API.

```json
{
  "version": "4",
  "groupKey": "soika:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222",
  "truncatedAlerts": 0,
  "status": "firing",
  "receiver": "soika",
  "groupLabels": {
    "project_id": "11111111-1111-4111-8111-111111111111",
    "issue_id": "22222222-2222-4222-8222-222222222222"
  },
  "commonLabels": {
    "alertname": "SoikaIssue",
    "project": "backend",
    "project_id": "11111111-1111-4111-8111-111111111111",
    "issue_id": "22222222-2222-4222-8222-222222222222",
    "severity": "error"
  },
  "commonAnnotations": {
    "summary": "ValueError: boom",
    "trigger": "new_issue",
    "event_count": "1"
  },
  "externalURL": "https://errors.example.com",
  "alerts": [{
    "status": "firing",
    "labels": {"alertname": "SoikaIssue", "project": "backend", "project_id": "11111111-1111-4111-8111-111111111111", "issue_id": "22222222-2222-4222-8222-222222222222", "severity": "error"},
    "annotations": {"summary": "ValueError: boom", "trigger": "new_issue", "event_count": "1"},
    "startsAt": "2026-10-30T06:00:00Z",
    "endsAt": "0001-01-01T00:00:00Z",
    "generatorURL": "https://errors.example.com/projects/abc123/issues/def456",
    "fingerprint": "22222222222242228222222222222222"
  }]
}
```

`project` is the project slug; `severity` is the Sentry level (omitted if absent).
Labels and annotations contain only strings. `commonLabels`/`commonAnnotations`
equal the single alert's maps. `trigger` is `new_issue` or `regression`;
`startsAt` is the first occurrence for a new issue, the latest for a regression.
`endsAt` is the zero timestamp. `groupKey` and `fingerprint` remain stable per issue.

The endpoint should return `2xx` within **5 seconds**. Delivery is attempted once;
failures are logged without retries or failing event ingestion.
