# Alertmanager integration

Soika sends new issues and regressions directly to Alertmanager's
`/api/v2/alerts` API and refreshes active alerts until they end.
Manually reopening a resolved or muted issue also starts a new alert when the
integration and channel are enabled and the project is not muted or orphaned.
Repeatedly reopening an already unresolved issue does not restart its alert.

## Connect Soika to Alertmanager

Set the following environment variables and restart Soika:

```dotenv
NOTIFICATIONS_ALERTMANAGER_ENABLED=true
ALERTMANAGER_URL=http://alertmanager.monitoring:9093
BASE_URL=https://errors.example.com
SERVICE_MANAGEMENT_SOURCE=env
ALERTMANAGER_DEFAULT_INTERVAL_SECONDS=300
ALERTMANAGER_DEFAULT_SEVERITY=warning
ALERTMANAGER_DEFAULT_LABELS=group=soika,chat=service-general-status
```

Supply the Alertmanager base URL without `/api/v2/alerts`. HTTP(S) and
reverse-proxy path prefixes are supported; HTTP authentication is not.
`BASE_URL` supplies public links to Soika issues. `ALERTMANAGER_SKIP_TLS=true`
disables certificate verification for this connection; its default is `false`.

In env mode, these settings apply at startup and notification controls are
read-only. There is no additional UI channel toggle to enable.
`chat` and `group` are custom labels sent with each alert.

For UI management, use `SERVICE_MANAGEMENT_SOURCE=ui` instead. As an Owner or
Manager, open **Admin → Notifications**, enable the Alertmanager channel and
save. An empty channel URL uses `ALERTMANAGER_URL`. Set the global interval,
severity, labels and annotations in the same screen; saved UI changes apply
without a restart. Environment defaults for interval and severity apply only
in env mode.

Use **Send test alert** in the monitored project's **Settings** to verify its
saved global and project labels, effective severity, and annotation templates.
The test alert's `alertname` uses the saved **Project test alert name** in UI
service management mode, or `ALERTMANAGER_TEST_ALERT_KIND` in env mode (default
`SoikaIssueCritical`, also when empty). This overrides the configured `alertname`
template; severity still follows the project settings. The general admin connectivity test uses `SoikaTestAlert`.
Project admins can send tests when the feature and channel are enabled, including
with configuration-managed settings. Sample event data is rendered with the real
project and team identity; the link opens the project. The alert has `test=true`,
a synthetic issue identity, and expires after five minutes. No issue or active
alert record is created. A test can be sent even when the project is muted.

The general connectivity test remains available in **Admin → Notifications**;
it uses fixed test values without project routing. Enabling the channel does
not send existing issues retroactively.

## Complete YAML example

Global connection settings and defaults belong in the environment block above.
To manage project overrides through YAML, also set:

```dotenv
PROJECT_MANAGEMENT_SOURCE=yaml
TEAM_MANAGEMENT_SOURCE=yaml
CONFIG_PATH=./soika-config.yaml
```

Example `soika-config.yaml`:

```yaml
version: 1
teams:
  - name: Platform
    members: []
projects:
  - slug: backend-api
    name: Backend API
    team: Platform
    alertmanager_interval_seconds: 120
    alertmanager_severity: critical
  - slug: frontend
    name: Frontend
    team: Platform
    alertmanager_severity: warning
  - slug: worker
    name: Worker
    team: Platform
    alertmanager_severity: info
```

Restart Soika after changing YAML. `CONFIG_PATH` is relative to the service's
working directory; in a container, mount the file and use its container path.
Merge these entries into the existing configuration: YAML management reconciles
all projects and teams according to the configured
[orphan policies](OPERATIONS.md#configuration-as-code-gitops).
The commented [config.example.yaml](../config.example.yaml) contains more options.

## Global and per-project refresh intervals

The global refresh interval follows `SERVICE_MANAGEMENT_SOURCE=ui|env`.
Use **Admin → Notifications** in UI mode or
`ALERTMANAGER_DEFAULT_INTERVAL_SECONDS` in env mode. The default is `60` seconds;
valid values are integers from `10` to `86400` seconds.

Project overrides follow `PROJECT_MANAGEMENT_SOURCE=ui|yaml`. Project admins
can change the interval in **Project → Settings** in UI mode. In YAML mode,
set `alertmanager_interval_seconds` on the project as above and restart.
An omitted or null override inherits the global interval.

Soika checks tracked alerts every 10 seconds. New issues and regressions are
sent immediately; failed deliveries are retried. Resolution, mute, deletion
and channel disablement end tracked alerts on the next check, independently
of the refresh interval. Alert leases expire after the greater of five minutes
or three refresh intervals if Soika stops refreshing them.
See [delivery lifecycle](NOTIFICATIONS.md#alertmanager).

## Per-project severity

The three supported classes are `critical`, `warning` and `info`. The global
default is `critical`. Set `ALERTMANAGER_DEFAULT_SEVERITY` in env mode or change
the default in **Admin → Notifications** in UI mode.

Project admins can select a class in **Project → Settings** when
`PROJECT_MANAGEMENT_SOURCE=ui`. With YAML management, set
`alertmanager_severity` on the project and restart. An omitted or null project
value inherits the global default. A project override takes precedence over the
global default and notification label templates.

### Severity mode

Choose **Fixed severity** (the default) or **From event level** in the global
severity card under **Admin → Notifications**. A project can inherit the global
mode or select either mode in **Project → Settings**.

With `SERVICE_MANAGEMENT_SOURCE=env`, set `ALERTMANAGER_SEVERITY_MODE=fixed|event`
and restart. With `PROJECT_MANAGEMENT_SOURCE=yaml`, set the project's
`alertmanager_severity_mode: fixed|event` and restart. UI changes apply without
a restart. Configuration-managed controls remain visible and read-only.

```yaml
projects:
  - slug: api
    name: API
    team: Platform
    alertmanager_severity_mode: event
    alertmanager_severity: warning # Fallback for missing or unknown levels.
```

| SDK event level | Alertmanager severity |
| --- | --- |
| `fatal`, `error` | `critical` |
| `warning` | `warning` |
| `info`, `debug` | `info` |
| Missing or unknown | Project fallback, then global default, then `critical` |

The original SDK level is preserved. An issue stores the highest mapped severity
observed over its lifetime; lower-level events never downgrade it. The peak
survives event retention, resolution, regressions and fingerprint merges.
At upgrade, existing issues initialize their peak from their saved issue level;
previous historical events are not rescanned. Subsequent events update the peak
in both modes, so switching modes uses the accumulated value.

Alertmanager receives issue alerts under the existing notification and mute
rules. Recurring events update the peak; the next lifecycle reconciliation
(normally within ten seconds) resolves an alert whose severity changed and sends
its replacement, independently of the project's refresh interval. Existing
issues without tracked alerts do not receive retroactive notifications.
Preview and project test alerts use a representative `error` level in event mode;
project tests retain the configured test alert name.

Existing configurations keep fixed severity. A project with an explicit severity
and no mode selects fixed mode for compatibility. Omitting both project fields
(or setting both to null) inherits the global mode and value. In event mode,
`alertmanager_severity` / `ALERTMANAGER_DEFAULT_SEVERITY` only supply the fallback
for an issue with no recognized levels. Fixed mode retains the legacy template
fallback when the global severity is cleared and no project value is set.

For regular issue alerts, the effective class also sets `alertname`, overriding
the configured label template (including an existing `SoikaIssue` value):

| Severity | Alert name |
| --- | --- |
| `critical` | `SoikaIssueCritical` |
| `warning` | `SoikaIssueWarning` |
| `info` | `SoikaIssueInfo` |

With `SERVICE_MANAGEMENT_SOURCE=ui`, edit **Admin → Notifications → Alert names**.
Owners and Managers can save names for all three classes and project test alerts.
Names must not be blank; surrounding whitespace is trimmed. Changes persist in
the database and apply without a restart, independently of label-template ownership.
The defaults above apply until edited. Environment name overrides are ignored
in UI mode. Switching to env mode preserves the saved UI names for later use.

With `SERVICE_MANAGEMENT_SOURCE=env`, set the names through environment variables.
The admin panel displays these names as read-only:

```dotenv
ALERTMANAGER_SEVERITY_CRITICAL=SoikaIssueCritical
ALERTMANAGER_SEVERITY_WARNING=SoikaIssueWarning
ALERTMANAGER_SEVERITY_INFO=SoikaIssueInfo
```

Unset, empty or whitespace-only values use the defaults above; surrounding
whitespace is trimmed. Restart Soika after changing these variables. Tracked
alerts with changed names are resolved and replaced on the next reconciliation.
These three variables do not change severity values or test alert names;
`ALERTMANAGER_TEST_ALERT_KIND` controls project test names independently in env mode.

If the global default is cleared in the UI and the project has no override,
the notification severity template applies (`{{ level }}` by default), and
the configured `alertname` template is used.
For exactly three classes, keep an explicit global default or project override.
Changing severity resolves the old alert identity and sends a replacement.

## Issue metadata sent by Soika

Built-in annotations use `<project name>: <issue title>` as the summary. The
description includes the error title, location, environment, release, event count,
last-seen timestamp and issue URL. It remains useful when the event has no culprit.
The URL is included in the description for webhook receivers that display only
`summary` and `description`, such as the Pachca middleware.

Env-managed deployments receive these defaults on startup. Existing UI-managed
or YAML-managed annotation maps are preserved; to use the richer format, update
them in **Admin → Notifications** or the YAML configuration. For example:

```json
{
  "service_name": "{{ project_name }}",
  "issue_title": "{{ title }}",
  "summary": "{{ project_name }}: {{ title }}",
  "description": "Error: {{ title }}\nLocation: {{ culprit }}\nEnvironment: {{ environment }}\nRelease: {{ release }}\nEvents: {{ event_count }}\nLast seen: {{ last_seen }}\nOpen the issue and inspect the latest event and stack trace.\nSoika: {{ issue_url }}",
  "issue_url": "{{ issue_url }}",
  "trigger": "{{ trigger }}"
}
```

Custom annotations can be edited in the UI or supplied through
`notifications.alertmanager.annotations` in `CONFIG_PATH` when
`SERVICE_MANAGEMENT_SOURCE=env`. YAML entries override built-in annotations by key.
See [message templates and matching UI/YAML examples](NOTIFICATIONS.md#issue-message-format).

Soika always adds `issue_id` and `project_id` labels. Keep changing values such
as titles, counts and timestamps in annotations. See
[template variables](NOTIFICATIONS.md#templates-and-variables) for all available
metadata fields.

## VMRule example: `soika`

VMRule resources are evaluated by vmalert, separately from Soika's direct
Alertmanager delivery. This example requires an **external issue exporter**
providing a `soika_issue_active` gauge (`1` for an active issue, `0` otherwise)
with the labels used below. **Soika does not export this metric.** Replace the
expressions with your datasource's actual issue queries. These rules do not
transform alerts already posted by Soika.

The resource and rule group are named `soika`. There are three rules, one per
severity, with a shared service/issue title format. Each input series represents
one issue and must include `issue_id` and `project_id` to retain its identity.
The other labels supply service name, issue title, location, environment,
release and the public issue URL.

```yaml
apiVersion: operator.victoriametrics.com/v1beta1
kind: VMRule
metadata:
  name: soika
  namespace: monitoring
spec:
  groups:
    - name: soika
      interval: 60s
      rules:
        - alert: SoikaIssueCritical
          expr: soika_issue_active{severity="critical"} == 1
          labels:
            group: soika
            severity: critical
            chat: service-general-status
          annotations:
            summary: '{{ $labels.service_name }}: {{ $labels.issue_title }}'
            description: |-
              Error: {{ $labels.issue_title }}
              Location: {{ $labels.culprit }}
              Environment: {{ $labels.environment }}
              Release: {{ $labels.release }}
              Open the issue and inspect the latest event and stack trace.
              Soika: {{ $labels.issue_url }}
            issue_url: '{{ $labels.issue_url }}'

        - alert: SoikaIssueWarning
          expr: soika_issue_active{severity="warning"} == 1
          labels:
            group: soika
            severity: warning
            chat: service-general-status
          annotations:
            summary: '{{ $labels.service_name }}: {{ $labels.issue_title }}'
            description: |-
              Error: {{ $labels.issue_title }}
              Location: {{ $labels.culprit }}
              Environment: {{ $labels.environment }}
              Release: {{ $labels.release }}
              Open the issue and inspect the latest event and stack trace.
              Soika: {{ $labels.issue_url }}
            issue_url: '{{ $labels.issue_url }}'

        - alert: SoikaIssueInfo
          expr: soika_issue_active{severity="info"} == 1
          labels:
            group: soika
            severity: info
            chat: service-general-status
          annotations:
            summary: '{{ $labels.service_name }}: {{ $labels.issue_title }}'
            description: |-
              Error: {{ $labels.issue_title }}
              Location: {{ $labels.culprit }}
              Environment: {{ $labels.environment }}
              Release: {{ $labels.release }}
              Open the issue and inspect the latest event and stack trace.
              Soika: {{ $labels.issue_url }}
            issue_url: '{{ $labels.issue_url }}'

```

The VMAlert instance must select this resource and use the datasource supplying
the issue series. See the [VMRule reference](https://docs.victoriametrics.com/operator/resources/vmrule/).
