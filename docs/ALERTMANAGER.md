# Alertmanager integration and readable notifications

Soika sends new issues and regressions directly to `/api/v2/alerts`, then
periodically refreshes active alerts until they are resolved or muted.
Alertmanager handles grouping, routing and delivery to chat. vmalert can send
metric alerts from VMRule resources to the same Alertmanager:

```text
Sentry SDK → Soika ───────────────────────────→ Alertmanager → receiver → chat
Service metrics → VictoriaMetrics → vmalert ──→ Alertmanager
                                      ↑ VMRule
```

VMRule resources monitor metrics; they do not process Soika issues. The `hbb_*`
metrics below belong to happybirthday-bot and are not exported by Soika.
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
the global interval, project overrides, severity classes and routing labels.
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
alertmanager_interval_seconds: 300
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

`info` is also supported. Omit `alertmanager_severity` or set it to `null` to use
the notification label template (Sentry event level by default); omit a project
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
in **Admin → Notifications**, then save a global refresh interval of `300`.
In **backend-api → Settings**, disable **Use global interval**, save `120`, and
select and save severity `critical`. In **frontend → Settings**, keep **Use global
interval** enabled and select severity `warning`. These UI changes apply without
a restart. Project admins can change their project settings; the global interval
requires an instance Owner or Manager. Project settings in YAML are ignored in
this mode. Team management can retain its existing source.

## Global and per-project refresh intervals

Refresh cadence is owned by **`PROJECT_MANAGEMENT_SOURCE`**, independently of
`SERVICE_MANAGEMENT_SOURCE` and the notification channel's configuration source.
Intervals are integer seconds from **10 to 86400**, with a global default of
**60 seconds**. A project without an override inherits the global value.

- With `PROJECT_MANAGEMENT_SOURCE=ui` (default), Owners and Managers edit the
  global interval in **Admin → Notifications → Alertmanager refresh interval**.
  Project admins edit the override in **Project → Settings**; select **Use global
  interval** to remove it. Changes are persisted and take effect without restart.
- With `PROJECT_MANAGEMENT_SOURCE=yaml`, both controls are read-only. Set the
  global value at the root of `CONFIG_PATH` and overrides in project entries,
  then restart Soika. An omitted global value resets to 60; an omitted or null
  project value removes the override.

```yaml
version: 1
alertmanager_interval_seconds: 300
projects:
  - slug: happybirthday-bot
    name: happybirthday-bot
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
`effective_interval_seconds`. The global value cannot be null.

## Per-project severity

Choose `critical`, `warning` or `info` in **Project → Settings → Alertmanager
severity**. Project admins can edit this setting when
`PROJECT_MANAGEMENT_SOURCE=ui`; with `PROJECT_MANAGEMENT_SOURCE=yaml`, set
`alertmanager_severity` in the project entry of `CONFIG_PATH` and restart Soika.
These are Soika's supported classes; Alertmanager itself routes on label values
rather than enforcing a severity enum.

```yaml
version: 1
projects:
  - slug: happybirthday-bot
    name: happybirthday-bot
    team: platform
    alertmanager_severity: critical
  - slug: backend
    name: Backend
    team: platform
    alertmanager_severity: warning
```

An explicit project severity overrides `severity` from both the channel labels
and `notifications.alertmanager.project_labels`. It applies to new alerts and
refreshes without changing Sentry event levels or affecting email and webhooks.
Selecting **Use notification template** in the UI, or omitting/clearing the YAML
field, restores the existing labels (by default `{{ level }}`). Changing severity
resolves tracked alerts with their original labels and sends their replacements
on the next check, independently of the refresh interval.

```text
GET /api/projects/{id}/alertmanager-severity
PUT /api/projects/{id}/alertmanager-severity
    {"severity": "critical"}   # Project admin
    {"severity": null}         # Restore the notification template
```

GET requires project access; PUT follows `PROJECT_MANAGEMENT_SOURCE` and accepts
only `critical`, `warning`, `info` or null. The global channel configuration and
its management source do not control this project setting.

## Short summaries, details and actions

Soika's default summary is `{{ title }}`, which can contain a long HTTP error,
URL and JSON. A receiver that prints only summary will show that raw line.
Prefer a short service/problem summary, context and action in description,
and a separate link to the issue or runbook.

For example, replace annotations in **Admin → Notifications** with:

```json
{
  "summary": "{{ project_name }}: application error",
  "description": "Error: {{ title }}\nLocation: {{ culprit }}\nEnvironment: {{ environment }}\nRelease: {{ release }}\nEvents: {{ event_count }}\nLast seen: {{ last_seen }}\nOpen the issue and inspect the latest event and stack trace.\nSoika: {{ issue_url }}",
  "issue_url": "{{ issue_url }}",
  "trigger": "{{ trigger }}"
}
```

Example routing labels:

```json
{
  "alertname": "SoikaIssue",
  "project": "{{ project_slug }}",
  "job": "{{ project_slug }}",
  "severity": "{{ level }}",
  "chat": "service-general-status"
}
```

Soika always adds `issue_id` and `project_id`. Its severity uses Sentry levels
(`fatal`, `error`, `warning`, `info`, `debug`); the VMRule examples use `critical`
and `warning`. Account for both in routes or select an explicit project severity
as described above. Keep titles, counts and timestamps in annotations:
changing a label creates a different alert identity.

Soika supports literal `{{ variable }}` substitution only, without conditions,
filters or `$labels`. Templates apply across projects and cannot turn arbitrary
logs into a diagnosis such as “KeyHub rejected the signature”. Metric rules can
provide precise descriptions of known conditions, as shown below.

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
  group_by: [cluster, job, project, alertname]
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

Expected metric alert message:

```text
🔥 Active [critical]
happybirthday-bot: birthday cache has not been populated
Cluster: k8s-internal
Instance: 10.0.0.15:8080
No KeyHub fetch has succeeded for 30 minutes. Sending will be skipped.
Check KEYHUB_SECRET_KEY and KeyHub availability.
```

For Soika issues, “Closed” means the alert ended, which can also result from
mute, deletion or channel disablement. It does not establish service recovery.

## VMRule example: happybirthday-bot

Every description below ends with a link to the source Soika host. Replace
`https://errors.example.com` with your public `BASE_URL` and
`happybirthday-bot` with the actual Soika project slug before deploying.

A metric alert does not carry a Soika issue/event ID, and can fire without any
corresponding SDK event (for example, when a pod is down). The runnable examples
therefore link to the project's issues. When a rule has a known matching issue,
replace its final description line with a link copied from that issue:

```text
Soika: https://errors.example.com/projects/happybirthday-bot/issues/<issue_short_id>
```

Replace `<issue_short_id>` with the real public issue ID; this is a URL pattern,
not a deployable link. The issue page opens its latest event and lets engineers
navigate earlier events. Soika currently has no browser URL for selecting an
individual event by ID. Do not invent an event URL or attach an unrelated issue.
For alerts sent directly by Soika, `{{ issue_url }}` already identifies the
correct issue; the Soika annotation example above appends it to the body.
Keep these links in annotations so they do not change alert identity.


Check metric names, scrape labels and thresholds in your datasource before
applying this example. The input selector uses
`job="k8s-internal-happybirthday-bot"`; the rule label `job: happybirthday-bot`
sets the service name for routing. If several clusters share the same scrape
job, add their source cluster label to every selector and to aggregation and
matching keys.

Your VMAlert must select this resource through `ruleSelector` and
`ruleNamespaceSelector`; add any required metadata labels. It must query the
appropriate datasource and notify the same Alertmanager. See the
[VMRule documentation](https://docs.victoriametrics.com/operator/resources/vmrule/)
and [VMAlert rule selection](https://docs.victoriametrics.com/operator/resources/vmalert/).

```yaml
apiVersion: operator.victoriametrics.com/v1beta1
kind: VMRule
metadata:
  name: happybirthday-bot
  namespace: monitoring
spec:
  groups:
    - name: happybirthday-bot
      interval: 60s
      rules:
        - alert: HBBDown
          expr: up{job="k8s-internal-happybirthday-bot"} == 0
          for: 10m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: pod is not responding to scrapes'
            description: |-
              Instance {{ $labels.instance }} has been unreachable for more than 10 minutes.
              Check pod status, the port, scrape annotations and /healthz.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBCacheNeverPopulated
          expr: hbb_cache_ready{job="k8s-internal-happybirthday-bot"} == 0
          for: 30m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: birthday cache has not been populated'
            description: |-
              No KeyHub fetch has succeeded since {{ $labels.instance }} started.
              Sending will be skipped with outcome not_ready.
              Check KEYHUB_SECRET_KEY, KEYHUB_BASE_URL and KeyHub availability.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBKeyHubStale
          expr: |
            (time() - hbb_keyhub_last_success_timestamp_seconds{job="k8s-internal-happybirthday-bot"}) > 6 * 3600
            and
            hbb_keyhub_last_success_timestamp_seconds{job="k8s-internal-happybirthday-bot"} > 0
          for: 15m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: KeyHub data is stale'
            description: |-
              The last successful fetch on {{ $labels.instance }} was more than 6 hours ago.
              New and departed employees are not reflected. Check fetch errors and KeyHub availability.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBKeyHubFetchErrors
          expr: increase(hbb_keyhub_fetch_total{job="k8s-internal-happybirthday-bot",result="error"}[2h]) >= 2
          for: 5m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: repeated KeyHub fetch failures'
            description: |-
              Fetching on {{ $labels.instance }} failed at least twice in 2 hours.
              The previous cache is retained. Inspect KeyHub request logs.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBKeyHubUnauthorized
          expr: increase(hbb_keyhub_http_requests_total{job="k8s-internal-happybirthday-bot",code=~"401|403"}[1h]) > 0
          for: 5m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: KeyHub rejects requests ({{ $labels.code }})'
            description: |-
              KeyHub returned {{ $labels.code }} on {{ $labels.instance }}.
              Check KEYHUB_SECRET_KEY, KEYHUB_APP_ID and request signature rules.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBKeyHubHTTPErrors
          expr: increase(hbb_keyhub_http_requests_total{job="k8s-internal-happybirthday-bot",code!~"200|401|403"}[1h]) > 0
          for: 15m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: KeyHub HTTP errors ({{ $labels.code }})'
            description: |-
              Requests on {{ $labels.instance }} failed with code {{ $labels.code }} in the last hour.
              code="error" means a network error or timeout. Check KeyHub and network policies.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBKeyHubSlow
          expr: |
            increase(hbb_keyhub_fetch_duration_seconds_sum{job="k8s-internal-happybirthday-bot"}[6h])
            /
            increase(hbb_keyhub_fetch_duration_seconds_count{job="k8s-internal-happybirthday-bot"}[6h]) > 30
          for: 30m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: KeyHub fetches are slow'
            description: |-
              The average full fetch duration over 6 hours on {{ $labels.instance }} exceeds 30 seconds.
              Updates may exceed the 2-minute cycle timeout. Check KeyHub latency.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBSendFailed
          expr: increase(hbb_pachca_send_total{job="k8s-internal-happybirthday-bot",result="error"}[15m]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: failed to send a birthday message to Pachca'
            description: |-
              Sending on {{ $labels.instance }} failed; the deduplication marker was not set.
              A retry may occur on restart or at the next SEND_AT; today's message may be missed.
              Check the Pachca response, token and bot channel permissions.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBPachcaAuthError
          expr: increase(hbb_pachca_send_errors_total{job="k8s-internal-happybirthday-bot",kind="auth"}[24h]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: Pachca authentication failed'
            description: |-
              Pachca returned an authentication error.
              Check PACHCA_TOKEN and bot access to PACHCA_CHANNEL_ID.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBRenderError
          expr: increase(hbb_congratulate_total{job="k8s-internal-happybirthday-bot",outcome="render_error"}[1h]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: failed to render the birthday message'
            description: |-
              There are birthdays to announce, but the message could not be rendered.
              Check phrases.yaml in the ConfigMap and the rendering error in logs.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBSendSkippedNotReady
          expr: increase(hbb_congratulate_total{job="k8s-internal-happybirthday-bot",outcome="not_ready"}[1h]) > 0
          for: 5m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: sending skipped because the cache was not ready'
            description: |-
              The birthday cache on {{ $labels.instance }} was empty at send time; no message was sent.
              Check KeyHub fetch errors.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBDailyRunMissing
          expr: |
            sum by (instance) (increase(hbb_congratulate_total{job="k8s-internal-happybirthday-bot"}[26h])) == 0
            and on (instance)
            (time() - hbb_start_time_seconds{job="k8s-internal-happybirthday-bot"}) > 26 * 3600
          for: 30m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: daily birthday cycle did not run'
            description: |-
              No send outcome has been recorded in 26 hours on {{ $labels.instance }}.
              The pod is older than 26 hours. Check the scheduler, logs and SEND_AT/APP_TZ.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBCacheEmpty
          expr: |
            hbb_cache_ready{job="k8s-internal-happybirthday-bot"} == 1
            and
            hbb_cache_people{job="k8s-internal-happybirthday-bot"} == 0
          for: 30m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: birthday cache is empty'
            description: |-
              KeyHub requests succeed but no eligible employees remain ({{ $labels.instance }}).
              Check KEYHUB_BIRTHDAY_FIELD, KEYHUB_BIRTHDAY_FORMAT and KEYHUB_ACTIVE_ONLY.
              DEBUG logs contain reasons why records were skipped.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBCachePeopleDropped
          expr: |
            hbb_cache_people{job="k8s-internal-happybirthday-bot"}
            < 0.8 * (hbb_cache_people{job="k8s-internal-happybirthday-bot"} offset 24h)
          for: 30m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: employee count in the cache dropped'
            description: |-
              The employee count on {{ $labels.instance }} fell by more than 20% in a day (currently {{ $value | printf "%.0f" }}).
              Check bulk account suspension, filters and fetch completeness.
              Soika: https://errors.example.com/projects/happybirthday-bot

        - alert: HBBStateWriteErrors
          expr: increase(hbb_state_write_errors_total{job="k8s-internal-happybirthday-bot"}[1h]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: failed to save the send marker'
            description: |-
              The message was sent, but state could not be written to STATE_DIR ({{ $labels.instance }}).
              Restarting may send a duplicate. Check PVC capacity, permissions and mounts.
              Soika: https://errors.example.com/projects/happybirthday-bot
```

Selectors are scoped to this service, cluster is an explicit label, and the
HTTP warning excludes 401/403 because they have a dedicated critical rule.
VMRule annotations use vmalert syntax such as `{{ $labels.instance }}` and
`{{ $value | printf "%.0f" }}`; these expressions do not work in Soika templates.

`increase(...[1h]) > 0` means an error occurred within the history window, not
necessarily an ongoing failure. The alert may remain active until that error
leaves the window. `for` requires the condition to remain true for that duration.
`offset 24h` requires historical data. `up == 0` detects failed scrapes of an
existing target; detect targets removed from discovery separately, for example
with `absent(up{job="k8s-internal-happybirthday-bot"})` and a suitable `for`.
The daily-cycle rule assumes counter series are initialized; missing metrics
need separate monitoring.

Before deployment, check expressions in the datasource and validate rules with
your vmalert version. Verify rule loading, firing alerts in Alertmanager, readable
chat output and a separate resolved message. Escape inner `{{ ... }}` when
passing these files through Helm `tpl`.
