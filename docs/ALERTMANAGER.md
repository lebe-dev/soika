# Alertmanager integration and readable notifications

Soika sends new issues and regressions directly to `/api/v2/alerts`, then
periodically refreshes active alerts until they are resolved or muted.
Alertmanager handles grouping, routing and delivery to chat:

```text
Sentry SDK → Soika → Alertmanager → receiver → chat
```

The project [webhook](WEBHOOKS.md) is a separate delivery channel.

## Minimal setup

1. Set the following environment variables and restart Soika:

   ```dotenv
   NOTIFICATIONS_ALERTMANAGER_ENABLED=true
   ALERTMANAGER_URL=http://alertmanager.monitoring:9093
   BASE_URL=https://errors.example.com
   SERVICE_MANAGEMENT_SOURCE=ui
   ```

2. As an Owner or Manager, open **Admin → Notifications**, enable the Alertmanager
   channel and save. Supply a base URL without `/api/v2/alerts`; leaving the URL
   empty uses `ALERTMANAGER_URL`. `BASE_URL` supplies public issue links. HTTP(S)
   and reverse-proxy prefixes are supported; HTTP authentication is not.
3. Use **Send test alert**, then send a new issue through an SDK and check chat.
   The test uses fixed text: use Preview and a real new issue to check custom
   annotations.

With `SERVICE_MANAGEMENT_SOURCE=env`, environment variables configure the channel
and default labels; the channel form is read-only. Project labels can be loaded
from `CONFIG_PATH`. Custom annotations require UI or the existing legacy YAML
mode. See [notification settings and template variables](NOTIFICATIONS.md) and
[deployment configuration](OPERATIONS.md#notifications).

## Complete YAML example

The repository's commented [config.example.yaml](../config.example.yaml) includes
environment variables for global defaults, project overrides, severity classes
and routing labels.
For a local binary, copy it into your configuration file:

```sh
cp config.example.yaml ./soika-config.yaml
```

Add these settings to the service environment (or `.env` for local development),
replacing the Alertmanager and public Soika URLs:

```dotenv
NOTIFICATIONS_ALERTMANAGER_ENABLED=true
ALERTMANAGER_URL=http://alertmanager.monitoring:9093
BASE_URL=https://errors.example.com
SERVICE_MANAGEMENT_SOURCE=env
ALERTMANAGER_DEFAULT_INTERVAL_SECONDS=300
ALERTMANAGER_DEFAULT_SEVERITY=warning
PROJECT_MANAGEMENT_SOURCE=yaml
TEAM_MANAGEMENT_SOURCE=yaml
CONFIG_PATH=./soika-config.yaml
ALERTMANAGER_DEFAULT_LABELS=chat=service-general-status
```

`CONFIG_PATH` is resolved from the service's working directory. In a container,
mount the file and set `CONFIG_PATH` to its path inside the container. Keep the
deployment's existing secret key, database and other required settings.

The following is a complete minimal alternative to the commented example file:

```yaml
version: 1
teams:
  - name: Platform
    members: []
  - name: Web
    members: []
projects:
  - slug: backend-api
    name: Backend API
    team: Platform
    alertmanager_interval_seconds: 120
    alertmanager_severity: critical
  - slug: frontend
    name: Frontend
    team: Web
    alertmanager_severity: warning
```

| Project | Refresh interval | Alertmanager severity |
| --- | --- | --- |
| `backend-api` | 120 seconds (project override) | `critical` |
| `frontend` | 300 seconds (global value) | `warning` |

`info` is also supported. Omit `alertmanager_severity` or set it to `null` to inherit
the global severity; when that is unset, the notification label template applies
(Sentry event level by default). Omit a project
interval or set it to `null` to inherit the global interval.

Restart Soika after editing YAML or environment variables. Env mode enables the
channel from `NOTIFICATIONS_ALERTMANAGER_ENABLED` and uses `ALERTMANAGER_URL`;
there is no additional UI channel toggle to enable. Configure an Alertmanager
route for `chat="service-general-status"` as shown [below](#chat-formatting-and-status).
Send a new SDK issue to each project to check the resulting labels. The admin
test alert uses fixed test labels and does not exercise project overrides.

YAML project/team management reconciles all projects and teams: entries missing
from the file follow the configured orphan policies. Merge existing entities
into your real configuration before enabling these sources; see
[GitOps behavior](OPERATIONS.md#configuration-as-code-gitops).

### The same settings through the UI

Set `PROJECT_MANAGEMENT_SOURCE=ui` and `SERVICE_MANAGEMENT_SOURCE=ui`, keep the
Alertmanager feature flag and URL above, and restart. Enable and save the channel
in **Admin → Notifications**, then save a global refresh interval of `300` and
default severity `warning`.
In **backend-api → Settings**, disable **Use global interval**, save `120`, and
select and save severity `critical`. In **frontend → Settings**, keep **Use global
interval** enabled and select severity `warning`. These UI changes apply without
a restart. Project admins can change their project settings; the global interval
requires an instance Owner or Manager. Project settings in YAML are ignored in
this mode. Team management can retain its existing source.

## Global and per-project refresh intervals

Global defaults are owned by **`SERVICE_MANAGEMENT_SOURCE=ui|env`**.
Project overrides are owned separately by **`PROJECT_MANAGEMENT_SOURCE=ui|yaml`**.
Intervals are integer seconds from **10 to 86400**, with a built-in default of
**60 seconds**. A project without an override inherits the global value.

| Setting | Environment mode (`SERVICE_MANAGEMENT_SOURCE=env`) | UI mode (`SERVICE_MANAGEMENT_SOURCE=ui`) |
| --- | --- | --- |
| Global refresh interval | `ALERTMANAGER_DEFAULT_INTERVAL_SECONDS` (default `60`) | Admin → Notifications → Alertmanager refresh interval |
| Global severity | `ALERTMANAGER_DEFAULT_SEVERITY` (default `critical`; `warning` and `info` also supported) | Admin → Notifications → Default Alertmanager severity |

In env mode, restart Soika to apply changes; both global controls are read-only.
In UI mode, saved defaults persist in the database across restarts and environment
values for these defaults are ignored. Switching from env to UI preserves the
last applied defaults as editable values. Switching to env replaces saved
defaults on startup. A fresh installation in UI mode starts with 60 seconds and
`critical` as the default severity. An unset or empty
`ALERTMANAGER_DEFAULT_SEVERITY` selects `critical` only in env mode. UI mode uses
the persisted database value, including an explicit admin choice to use the
notification template.

Project admins edit interval and severity overrides in **Project → Settings**
when `PROJECT_MANAGEMENT_SOURCE=ui`. UI edits take effect without restart.
With `PROJECT_MANAGEMENT_SOURCE=yaml`, edit only project entries in `CONFIG_PATH`
and restart. Omitted or null project values inherit the global defaults.
Global defaults never belong in the YAML file: remove the former root
`alertmanager_interval_seconds` field and set `ALERTMANAGER_DEFAULT_INTERVAL_SECONDS`
instead. The old root field is rejected as unknown, including in UI mode.

For example, with `ALERTMANAGER_DEFAULT_INTERVAL_SECONDS=300` and
`ALERTMANAGER_DEFAULT_SEVERITY=warning` in env mode:

```yaml
version: 1
projects:
  - slug: backend-api
    name: Backend API
    team: platform
    alertmanager_interval_seconds: 120
    alertmanager_severity: critical
  - slug: backend
    name: Backend
    team: platform
    # Inherits 300 seconds.
```

The owning team must already exist or be declared through the existing team
management configuration. In UI mode these YAML interval settings are ignored.

Soika checks tracked alerts every 10 seconds and sends a refresh when the
interval since the last successful send has elapsed. The first alert is sent
immediately for a new issue or regression. Failed sends are retried on the next
check, and the last successful send time survives restarts. Resolution, mute,
deletion, project orphaning and channel disablement bypass the refresh interval;
under normal delivery, the resolved payload is sent on the next check.
Enabling the channel does not send existing issues retroactively.

Firing payloads expire after the greater of five minutes or three refresh
intervals. Increasing an interval extends an existing short lease on the next
check. If Soika stops or the global feature flag is disabled, alerts expire
when their last lease ends. Disabling the channel while the feature remains
enabled sends explicit resolved payloads. See [delivery lifecycle](NOTIFICATIONS.md#alertmanager).

**Refresh cadence is not chat frequency.** Alertmanager's `repeat_interval`,
`group_interval`, silences and receiver configuration control chat delivery.
Refreshing an unchanged alert keeps it active without forcing another message.

The interval API uses the same permissions and ownership rules:

```text
PUT /api/settings/notifications/interval
    {"interval_seconds": 300}                 # Instance Owner or Manager
GET /api/projects/{id}/alertmanager-interval   # Project member
PUT /api/projects/{id}/alertmanager-interval
    {"interval_seconds": 120}                 # Project admin
    {"interval_seconds": null}                # Remove project override
```

Responses include `interval_seconds` (null for an inheriting project) and
`effective_interval_seconds`. The global value cannot be null. The global PUT
requires UI service management; the project PUT requires UI project management.

## Per-project severity

The initial global severity is **`critical`** in both management modes. In env
mode, an unset or empty `ALERTMANAGER_DEFAULT_SEVERITY` selects `critical`; set
`warning` or `info` explicitly to change it. In UI mode, `critical` is the
initial database default and admins can change or clear it in **Admin →
Notifications**. Their choice persists across restarts. The upgrade initializes
previously empty database defaults to `critical`, preserving explicit classes.


Choose `critical`, `warning` or `info` in **Project → Settings → Alertmanager
severity**. Project admins can edit this setting when
`PROJECT_MANAGEMENT_SOURCE=ui`; with `PROJECT_MANAGEMENT_SOURCE=yaml`, set
`alertmanager_severity` in the project entry of `CONFIG_PATH` and restart Soika.
These are Soika's supported classes; Alertmanager itself routes on label values
rather than enforcing a severity enum.

```yaml
version: 1
projects:
  - slug: backend-api
    name: Backend API
    team: platform
    alertmanager_severity: critical
  - slug: backend
    name: Backend
    team: platform
    alertmanager_severity: warning
```

An explicit project severity overrides the global default, then `severity` from
both the channel labels and `notifications.alertmanager.project_labels`. It applies to new alerts and
refreshes without changing Sentry event levels or affecting email and webhooks.
Selecting **Use global default** in the project UI, or omitting/clearing the YAML
field, restores the global default severity. If the global default is unset, the
existing label templates apply (by default `{{ level }}`). Clearing the global
default in the admin UI also restores that template fallback. Changing severity
resolves tracked alerts with their original labels and sends their replacements
on the next check, independently of the refresh interval.

```text
GET /api/projects/{id}/alertmanager-severity
PUT /api/projects/{id}/alertmanager-severity
    {"severity": "critical"}   # Project admin
    {"severity": null}         # Inherit the global severity
```

GET requires project access; PUT follows `PROJECT_MANAGEMENT_SOURCE` and accepts
only `critical`, `warning`, `info` or null. The service management source controls
the global default, while project management controls only the project override.

Instance Owners and Managers can set the default with
`PUT /api/settings/notifications/severity` and the same JSON body when
`SERVICE_MANAGEMENT_SOURCE=ui`; `null` clears the global default.
`GET /api/settings/notifications` includes `default_severity` and the global
interval. In env mode these global write endpoints return `403`.

## The `soika` notification group

Use `soika` as the shared group label and three severity classes across services:

| Group | Severity | Title template |
| --- | --- | --- |
| `soika` | `critical` | `{{ project_name }}: {{ title }}` |
| `soika` | `warning` | `{{ project_name }}: {{ title }}` |
| `soika` | `info` | `{{ project_name }}: {{ title }}` |

Each issue produces an alert with its effective project severity; the three
rows are classes, not three copies of every issue. All classes use the same
annotation templates below. Alertmanager groups by `group`, `severity` and
`project` so different services and severity classes remain separate.

Soika currently sends these alerts directly to Alertmanager. It does not export
issue metrics for vmalert. A Kubernetes `VMRule` named `soika` with three rules
would require an additional datasource containing the issues and their severity
and context. The configuration below applies to the existing direct integration.

## Short summaries, details and actions

Use a shared title template for all three severity classes:
`{{ project_name }}: {{ title }}`. Here `project_name` is the service's Soika
project name and `title` is the issue title.
Keep the error location, environment, release, event count and issue link in the
description.

For example, replace annotations in **Admin → Notifications** with:

```json
{
  "summary": "{{ project_name }}: {{ title }}",
  "description": "Error: {{ title }}\nLocation: {{ culprit }}\nEnvironment: {{ environment }}\nRelease: {{ release }}\nEvents: {{ event_count }}\nLast seen: {{ last_seen }}\nOpen the issue and inspect the latest event and stack trace.\nSoika: {{ issue_url }}",
  "issue_url": "{{ issue_url }}",
  "trigger": "{{ trigger }}"
}
```

Example routing labels:

```json
{
  "alertname": "SoikaIssue",
  "group": "soika",
  "project": "{{ project_slug }}",
  "job": "{{ project_slug }}",
  "severity": "{{ level }}",
  "chat": "service-general-status"
}
```

Soika always adds `issue_id` and `project_id`. Use the global and per-project
severity settings above to select `critical`, `warning` or `info`. When both
settings are cleared, the `{{ level }}` label template falls back to Sentry event
levels (`fatal`, `error`, `warning`, `info`, `debug`), which do not match this
three-class scheme.
Keep titles, counts and timestamps in annotations: changing a label creates a
different alert identity.

Soika supports literal `{{ variable }}` substitution only, without conditions,
filters or `$labels`. See [template variables](NOTIFICATIONS.md#templates-and-variables)
for all available fields.

## Chat formatting and status

Render description and the issue link, and read status from each alert rather
than prefixing every summary with a fixed green check mark. The group `.Status`
is firing if any member is firing; inside `range .Alerts`, `.Status` belongs to
that individual alert. See the
[Alertmanager template reference](https://prometheus.io/docs/alerting/latest/notifications/).

Example `/etc/alertmanager/templates/readable.tmpl`:

```gotemplate
{{ define "readable.message" }}
{{ range .Alerts }}
{{ if eq .Status "firing" }}🔥 Active{{ else }}✅ Closed{{ end }} [{{ .Labels.severity }}]
{{ .Annotations.summary }}
{{ with .Labels.cluster }}Cluster: {{ . }}{{ end }}
{{ with .Labels.instance }}Instance: {{ . }}{{ end }}
{{ with .Annotations.description }}{{ . }}{{ end }}
{{ if .Annotations.issue_url }}Issue: {{ .Annotations.issue_url }}{{ else if .GeneratorURL }}Source: {{ .GeneratorURL }}{{ end }}
{{ with .Annotations.runbook_url }}Runbook: {{ . }}{{ end }}
{{ end }}
{{ end }}
```

Load the file through `templates` and invoke `{{ template "readable.message" . }}`
in a receiver's text field. For example, with Slack:

```yaml
templates:
  - /etc/alertmanager/templates/*.tmpl
route:
  receiver: fallback
  group_by: [group, severity, project, alertname]
  group_wait: 30s
  group_interval: 5m
  repeat_interval: 4h
  routes:
    - receiver: service-general-status
      matchers:
        - 'chat="service-general-status"'
receivers:
  - name: fallback
    # Add your receiver for alerts without a matching chat label.
  - name: service-general-status
    slack_configs:
      - api_url_file: /etc/alertmanager/secrets/slack-webhook-url
        channel: '#service-general-status'
        send_resolved: true
        title: 'Service notifications'
        text: '{{ template "readable.message" . }}'
```

Merge this into your Alertmanager configuration. The example fallback drops
messages until you add an integration. `chat` is a custom label: it needs a route
and does not select a destination automatically. See
[Alertmanager receiver configuration](https://prometheus.io/docs/alerting/latest/configuration/).

For webhook delivery to Pachca or a custom gateway, the gateway renders the
message from `alerts[].status`, `alerts[].annotations` and `alerts[].generatorURL`.
Alertmanager `.tmpl` files do not change webhook JSON. Update the gateway if it
currently prints only summary or always adds a green check mark. Iterate over
`alerts[]`: `commonAnnotations` contains only values shared by every alert, so
individual descriptions may be absent there.

Expected Soika issue message:

```text
🔥 Active [critical]
Backend API: ValueError: invalid order status
Error: ValueError: invalid order status
Location: app.checkout
Environment: production
Release: 1.0.0
Events: 42
Last seen: 2026-01-01T12:00:00Z
Open the issue and inspect the latest event and stack trace.
Soika: https://errors.example.com/projects/backend-api/issues/iss001
```

For Soika issues, “Closed” means the alert ended, which can also result from
mute, deletion or channel disablement. It does not establish service recovery.
