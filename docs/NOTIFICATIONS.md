# Notifications

**Admin → Notifications** groups Email/SMTP, project Webhooks and Alertmanager.
Instance Owners and Managers can view and configure notifications. Personal email
opt-out remains in the profile; project and issue mute suppress all channels.
Email delivery, invite mail and the SMTP test action retain their existing
`SMTP_*` configuration. Webhooks retain `WEBHOOKS_ENABLED` and each project's URL;
see [webhook delivery](WEBHOOKS.md).

## Alertmanager

This channel sends an array of alerts to the Prometheus Alertmanager
[`POST /api/v2/alerts` API](https://github.com/prometheus/alertmanager/blob/main/api/v2/openapi.yaml).
Supply the **base URL**, for example `http://alertmanager.monitoring:9093`.
Set `ALERTMANAGER_URL` to supply a default base URL from the environment. An
explicit URL saved through the UI or supplied in YAML takes precedence. Leaving
`url` empty uses the environment default without storing a copy: later changes
to the environment take effect after restart. If the channel is enabled, either
an explicit URL or `ALERTMANAGER_URL` is required. The default URL is displayed
as a hint in the UI; editing or clearing the override is available in UI mode.
In YAML mode the UI remains read-only, including the URL.

A reverse-proxy prefix is supported (`https://monitoring.example.com/alertmanager`).
Soika appends `/api/v2/alerts`. HTTP and HTTPS are supported, with normal certificate
verification. URL credentials, query strings and fragments are rejected; this
version supports endpoints without HTTP authentication. Redirects are not followed.

The feature is disabled by default: set
`NOTIFICATIONS_ALERTMANAGER_ENABLED=true` and restart Soika before enabling the
channel in UI/YAML. This environment feature flag takes precedence over
`alertmanager.enabled`. When false, **Admin → Notifications** displays a compact
“Disabled in configuration” message; editing and preview API calls return `403`.
Soika sends no Alertmanager requests, including refreshes and resolutions, while
the flag is off. Previously sent alerts expire after their five-minute TTL;
saved records are retained and reconciled when the feature is enabled again.
Disabling the channel through UI/YAML while the feature remains enabled still
sends resolved payloads as described below. Configuration edits cannot enable
the global feature flag.

Owners and Managers can use **Send test alert** whenever the global feature flag
is enabled, regardless of the management source or channel enabled state. The test uses the saved
endpoint and fixed English or Russian text
matching the interface language, independently of notification templates. It sends
`alertname=SoikaTestAlert`, `severity=info`, `test=true` and the Soika base URL as
`instance`. It creates no issue or refresh record and expires after five minutes.
Success means Alertmanager accepted the request; downstream delivery follows its
routing, grouping and silence rules. Transport failures are shown in the UI.
The message identifies common DNS, connection, timeout and TLS certificate
failures, or the HTTP status returned by Alertmanager, without including the
endpoint URL or response body. Unrecognized failures use a general description.
In UI mode, enter and save an explicit Alertmanager URL before testing. In env
and YAML modes the configured URL or environment fallback is used.

The channel is also disabled by default. Its configuration applies to every project,
independently of Email and Webhooks. Enabling it does not send old issues.

- A new issue or regression sends a firing alert, unless the project or issue is muted.
- Soika stores the alert identity before delivery and refreshes tracked unresolved
  alerts every 60 seconds. Each firing payload has `endsAt` five minutes ahead.
- Resolving, muting or deleting the issue, muting/deleting/orphaning the project,
  or disabling the channel sends a resolved payload on the next refresh. Under
  normal delivery this takes up to a minute. Auto-unmuting does not create a new
  alert by itself; a subsequent regression can create one.
- Delivery failures are logged and retried on the next refresh, without failing
  ingestion. HTTP requests time out after five seconds. Saved identities survive
  restarts. Resolved records are removed only after successful delivery.
- Template or URL changes close each tracked alert at its original destination
  with its original labels, then send its replacement. Keep the old destination
  reachable during this transition, including when disabling the channel.
- During an outage longer than five minutes, Alertmanager can expire an alert;
  it can fire again after delivery recovers. Alertmanager controls grouping,
  silencing, repeat intervals and downstream receivers.

The existing project Webhook is a separate channel: it sends Alertmanager's
*outgoing webhook* format to a receiver, rather than posting alerts into
Alertmanager. A profile email opt-out does not suppress either project channel.

## UI or YAML configuration

`SERVICE_MANAGEMENT_SOURCE=ui` (default) allows edits in **Admin → Notifications**.
Settings and templates are persisted in the database. The YAML `notifications`
section is ignored. Preview renders sample data without sending an alert.

Set `SERVICE_MANAGEMENT_SOURCE=env` to use environment variables instead.
At startup, `NOTIFICATIONS_ALERTMANAGER_ENABLED` sets the channel enabled state,
`ALERTMANAGER_URL` sets its URL and `ORGANIZATION_NAME` sets the organization name.
`ALERTMANAGER_DEFAULT_LABELS` adds or overrides labels using comma-separated `name=value`
pairs, for example `team=platform,environment=production`. Values support the same
templates as the UI, such as `service={{ project_slug }}`. Whitespace around names
and values is trimmed; commas cannot be used in values. Duplicate names and invalid
labels or templates prevent startup. Unspecified labels and annotations use built-in
defaults. Existing database values are replaced.
Project-specific labels can be set in `CONFIG_PATH`, keyed by project slug,
independently of whether projects are managed in the UI or YAML:

```yaml
version: 1
notifications:
  alertmanager:
    project_labels:
      api:
        team: platform
        severity: '{{ level }}'
      frontend:
        team: web
```

Project labels override defaults with the same name. They are loaded at startup;
restart to apply changes. A missing configuration file leaves project labels empty
when projects and teams are managed in the UI. The `notifications` section's other
fields are ignored in env mode.
The UI shows “Configured through environment variables”; notification editing,
and organization-name changes are blocked in the UI and API, including
for Owners. Restart Soika to apply environment changes. `yaml` and `yml` are not
valid values for this variable. Project and team management remain independent.

Legacy `NOTIFICATION_MANAGEMENT_SOURCE=yaml` remains supported only when
`SERVICE_MANAGEMENT_SOURCE` is unset. It loads the following configuration from
`CONFIG_PATH` at startup; notification editing is blocked.

```yaml
version: 1
notifications:
  alertmanager:
    enabled: true
    url: http://alertmanager.monitoring:9093
    labels:
      alertname: SoikaIssue
      project: '{{ project_slug }}'
      severity: '{{ level }}'
      service: backend
    annotations:
      summary: '{{ title }}'
      description: '{{ project_name }}: {{ culprit }}'
      issue_url: '{{ issue_url }}'
      trigger: '{{ trigger }}'
      environment: '{{ environment }}'
      release: '{{ release }}'
      event_count: '{{ event_count }}'
```

Omitted fields use defaults. Supplying a labels or annotations map replaces that
map in full; `alertname` is required. `notifications: {}` disables Alertmanager
with the default templates. `${ENV_NAME}` expansion is supported in YAML string
values, as with projects and teams. The UI displays the configured endpoint and
templates, so do not put credentials in template values.

Switching back to UI preserves the last applied YAML configuration as editable
settings. Switching to YAML replaces the stored configuration with the file's
values on startup.

## Templates and variables

Use `{{ variable }}` placeholders in label and annotation **values**. The UI
accepts JSON objects with string values; YAML uses ordinary mappings. Substitution
is literal and single-pass, then the complete payload is JSON encoded. There are
no loops, filters, expressions or includes. Unknown placeholders are rejected.
Map keys must match `[A-Za-z_][A-Za-z0-9_]*`; each map accepts up to 64 entries,
with keys up to 128 bytes and templates up to 8192 bytes.

| Variable | Value |
| --- | --- |
| `project_name` | Display name of the project |
| `project_slug` | Project slug |
| `project_id`, `team_id` | Project and owning team UUIDs |
| `issue_id` | Issue UUID |
| `issue_short_id` | Public issue ID used in the UI |
| `title`, `culprit` | Error title and location |
| `level` | Sentry severity, falling back to `error` when absent |
| `environment`, `release` | Issue environment and release |
| `event_count` | Current issue event count |
| `first_seen`, `last_seen` | RFC 3339 timestamps in UTC |
| `issue_url` | Public link using `BASE_URL` and short project/issue IDs |
| `org_name` | Current organization display name from service settings |
| `trigger` | `new_issue` or `regression`, retained for the alert lifetime |

Absent optional values render as empty strings. The preview uses representative
sample values, rather than issue data.

`issue_id` and `project_id` labels are always added by Soika and cannot be
overridden. Alertmanager identifies alerts by their complete label set. Prefer
stable routing labels; put changing values such as event count, title and trigger
in annotations. A changed rendered label causes the old identity to resolve and
a replacement to fire. Resolution always reuses the last saved label set.

See [operations](OPERATIONS.md#notifications) and the
[Helm chart](../helm-chart/README.md#notifications) for deployment configuration.

In `SERVICE_MANAGEMENT_SOURCE=env` mode, general service settings are read-only
in the UI and API for all roles. `ORGANIZATION_NAME` sets the organization name,
`ALLOW_SIGNUP` controls registration, and `DEFAULT_TEAM_IDS` supplies comma-separated
UUIDs of existing teams assigned to new users (empty by default). Environment
values apply at startup and replace saved settings. Project/team management,
user administration and mute retain their existing permission rules.
