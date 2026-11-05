# Alertmanager integration

Soika sends new issues and regressions directly to Alertmanager's
`/api/v2/alerts` API and refreshes active alerts until they end.

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

Use **Send test alert** in **Admin → Service settings** to check connectivity.
The button is available when the Alertmanager feature and saved channel are
enabled, including when configuration manages the settings. Test sending is
also available in **Admin → Notifications**. Then send a new SDK issue to
verify actual project labels, severity and annotations. The test alert uses
fixed test values. Enabling the channel does not send existing issues
retroactively.

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

If the global default is cleared in the UI and the project has no override,
the notification severity template applies (`{{ level }}` by default).
For exactly three classes, keep an explicit global default or project override.
Changing severity resolves the old alert identity and sends a replacement.

## Issue metadata sent by Soika

In **Admin → Notifications**, the following annotations produce a title of
`<service name>: <Soika issue title>` and retain the issue details:

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

Custom annotations require UI management or the supported legacy YAML mode;
there is no environment variable for custom annotations. Env mode uses the
built-in annotation defaults. See [notification configuration](NOTIFICATIONS.md#ui-or-yaml-configuration).

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
