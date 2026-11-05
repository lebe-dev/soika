# Soika Helm chart

This chart deploys the single-binary Soika image with SQLite or PostgreSQL 15+.
It uses one replica and defaults to the `Recreate` deployment strategy: the retention
scheduler, ingestion rate limiter, and login lockout are per process. Configure
`strategy` to change the Kubernetes Deployment strategy. A rolling update can
briefly run multiple processes; use `maxSurge: 0` to avoid overlap. The application runs
the selected database's migrations at startup; no migration Job is needed.
Chart version: `0.3.6`. Default application image: `tinyops/soika:0.10.1`.

## Publishing

Set a new `version` in `Chart.yaml` for each chart release. Run `just release-chart`
to lint, render, test, and package the chart with the version from `Cargo.toml`
as its `appVersion`. The command clones or updates the chart repository in
`helm-repo/`, adds the package and index, and commits them there. Publish the
prepared commit with `git -C helm-repo push`.

## SQLite install (default)

Create a local values file with a public URL and a persistent secret key:

```yaml
envs:
  - name: BIND_ADDR
    value: "0.0.0.0:8080"
  - name: DATABASE_URL
    value: "sqlite:///app/data/soika.db?mode=rwc"
  - name: BASE_URL
    value: "https://errors.example.com"
  - name: ADMIN_USERS
    value: "alice@example.com,bob@example.com"
secrets:
  envs:
    - name: SECRET_KEY
      value: "replace-with-a-random-secret"
ingress:
  enabled: true
  hosts:
    - host: errors.example.com
      paths:
        - path: /
          pathType: Prefix
  tls:
    - hosts: [errors.example.com]
      secretName: errors-tls
```

Generate a key with `openssl rand -hex 32`, keep the values file out of Git,
and install with:

```sh
helm upgrade --install soika ./helm-chart -f my-values.yaml
```

Alternatively, create a Kubernetes Secret with `SECRET_KEY` and set
`secrets.create=false` and `secrets.existingSecret=<name>`. Other sensitive
variables such as `SMTP_PASSWORD` and `OAUTH_CLIENT_SECRET` can go in the same
Secret. List non-sensitive variables in `envs`; the full set is documented in
the root `.env.example`. `values.yaml` lists the commonly used defaults and names the
remaining optional variables in comments. Helm replaces the whole `envs` list when you
override it, so copy the entries you want to keep.
When using an existing Secret, restart the Deployment after changing its data;
Helm can only trigger an automatic rollout for a Secret created by this chart.

The image defaults to `tinyops/soika` at the chart's `appVersion`. Use
`image.repository` and `image.tag` for a private registry. `BASE_URL` must
match the external URL for DSNs, email links, secure cookies, and passkeys.
Enable either `ingress` or `httpRoute` for public access. HTTPRoute requires
the Gateway API CRDs and a configured Gateway.

`ADMIN_USERS` grants the listed full email addresses the same effective `Owner`
permissions as the setup administrator, across password, OAuth/OIDC and passkey
sign-in. The default is empty; matching ignores ASCII letter case and surrounding
spaces. Authentication and account-approval checks still apply. The stored role
shown in **Admin → Users** stays independent of this grant.

`EMAIL_LOGIN_ENABLED` defaults to `"true"` in `envs`. Set it to `"false"` to
hide the email/password form and reject password sign-in for all accounts,
including Owners and `ADMIN_USERS`. Public registration and anonymous
password-based invite acceptance are disabled too. Configure SSO or passkeys
before disabling it; existing sessions and first-run setup remain available.
See [email login configuration](../docs/OPERATIONS.md#disable-emailpassword-sign-in).

SSO-пользователи по умолчанию не могут менять имя в профиле или через API.
Чтобы разрешить изменение, установите `SSO_ALLOW_DISPLAY_NAME_CHANGE` в `"true"`
в списке `envs` (по умолчанию `"false"`). Ограничение действует для OIDC и GitHub
SSO, включая владельцев; локальные учётные записи могут менять имя всегда.

Changing `envs` changes the ConfigMap checksum and automatically rolls out the
Deployment on `helm upgrade`. Removing an address restores its stored role,
including existing sessions, after the rollout. Helm replaces the entire `envs`
list when overriding it, so include all required entries in your values file.
See [administrator configuration](../docs/OPERATIONS.md#administrators-from-environment-configuration)
for the complete behavior.

For SQLite, the chart creates a `ReadWriteOnce` PVC at `/app/data`. Use a storage class
with working SQLite file locks; avoid NFS and SMB. Set
`persistence.existingClaim` to reuse a claim. The created PVC has Helm's
`keep` annotation so uninstall does not delete the database. Disabling
`persistence` uses an `emptyDir`, which loses data when the Pod is replaced.
The application also uses a writable temporary directory at `/tmp`.

## PostgreSQL install

Create a PostgreSQL 15+ database separately. Set its URL in a Secret because it
contains a password; replace the default SQLite `DATABASE_URL` in `envs` by
specifying the full list without it:

```yaml
envs:
  - name: BIND_ADDR
    value: "0.0.0.0:8080"
  - name: BASE_URL
    value: "https://errors.example.com"
  - name: ADMIN_USERS
    value: "alice@example.com,bob@example.com"
secrets:
  envs:
    - name: SECRET_KEY
      value: "replace-with-a-random-secret"
    - name: DATABASE_URL
      value: "postgres://soika:password@postgres.example.com:5432/soika?sslmode=require"
persistence:
  enabled: false
```

You can also set `secrets.create: false` and `secrets.existingSecret` to a
preexisting Secret containing both `SECRET_KEY` and `DATABASE_URL`. With
`persistence.enabled: false`, the chart creates no PVC; the `/app/data` mount
is an ephemeral `emptyDir` and does not hold application data on PostgreSQL.
The PostgreSQL database must exist before Soika starts; Soika creates its tables.
Switching from SQLite to PostgreSQL does not copy existing data. Ensure the
selected Soika image includes PostgreSQL support.

## Управление через конфигурацию (0.9.0)

`PROJECT_MANAGEMENT_SOURCE` и `TEAM_MANAGEMENT_SOURCE` в `envs[]` независимо
выбирают `ui` (по умолчанию) или `yaml`. Все четыре комбинации поддерживаются.
При `yaml` соответствующая секция файла обязательна. Пустые секции задаются явно.
Чарт монтирует `config.yaml` в `/etc/soika` как read-only volume без `subPath`;
изменение `configFile.content` меняет checksum и перезапускает pod.

```yaml
configFile:
  existingConfigMap: ""
  content:
    version: 1
    teams:
      - name: Platform
        members:
          - email: alice@example.com
            role: admin
        sso_groups:
          - group: acme/platform
            role: contributor
    projects:
      - slug: backend-api
        name: Backend API
        team: Platform
        retention_events: 5000
        retention_days: 30
        webhook_url: ${BACKEND_WEBHOOK_URL}
```

Для готового ConfigMap задайте `configFile.existingConfigMap`; он должен содержать
ключ `config.yaml`. Чарт не создаёт свой ConfigMap в этом режиме. При изменении
внешнего ConfigMap самостоятельно выполните rollout: checksum внешнего содержимого
чарту неизвестен. Секреты `${VAR}` передавайте через `secrets.envs[]` или existing
Secret. Не записывайте DSN в файл: он создаётся один раз и хранится в БД.

Состав команд по группам требует OIDC/SSO (GitLab); `OAUTH_GROUPS_CLAIM` выбирает
имя поля со списком групп, по умолчанию `groups`, а не имя самой группы.
Например, задайте `OAUTH_GROUPS_CLAIM=groups`, а `infra` укажите в
`teams[].sso_groups[].group` файла конфигурации. После изменения нужен повторный
SSO-вход. Без OAuth новые пользователи не смогут войти в
команды из файла; регистрация и приглашения отключены. Сопоставление групп точное,
без автоматического включения подгрупп; исключение из группы применяется при
следующем SSO-входе. Passkeys при включённом SSO должны быть выключены.

`PROJECT_ORPHAN_POLICY` и `TEAM_ORPHAN_POLICY` по умолчанию `readonly`: события в
исключённые проекты не принимаются, данные сохраняются. `delete` необратимо
удаляет отсутствующие сущности; команду с проектами удалить нельзя. Сначала
перечислите сохраняемые сущности, включите `yaml` с `readonly`, проверьте админку.
Пустой файл по умолчанию исключает **все** существующие сущности; предохранителя
от массового удаления нет. Смена slug означает новый проект и новый DSN.

Подробности: [Configuration as code](../docs/OPERATIONS.md#configuration-as-code-gitops).

## Notifications

Set `NOTIFICATIONS_ALERTMANAGER_ENABLED="true"` to enable the Alertmanager feature
(default `"false"`). `ALERTMANAGER_URL` is a base URL: Soika appends `/api/v2/alerts`.
With `SERVICE_MANAGEMENT_SOURCE=ui` (default), enable and save the channel in
**Admin → Notifications**. In env mode, the feature flag also sets the channel
state at startup, so no additional UI toggle is required.

| Variable | Chart default | Behavior |
| --- | --- | --- |
| `SERVICE_MANAGEMENT_SOURCE` | `"ui"` | Owns global defaults and channel settings: `ui` or `env` |
| `ALERTMANAGER_DEFAULT_INTERVAL_SECONDS` | `"60"` | Refresh cadence for projects without an override; range `10..86400` |
| `ALERTMANAGER_DEFAULT_SEVERITY` | `"critical"` | Default class: `critical`, `warning`, `info`; empty/unset also selects `critical` in env mode |
| `PROJECT_MANAGEMENT_SOURCE` | `"ui"` | Owns project overrides: `ui` or `yaml` |

In env mode, global defaults are applied on startup and read-only in the admin
panel. In UI mode, Owners and Managers edit them in **Admin → Notifications**;
the initial database severity is `critical`; saved admin choices survive restarts
and the environment defaults are ignored. Project
admins manage overrides in **Project → Settings** when projects are UI-managed,
or in `configFile.content.projects` with YAML project management. An explicit
project severity overrides the global default, then notification label templates.

### Environment defaults with YAML project overrides

Helm replaces the complete `envs` list. The following example includes the base
application settings; keep your existing persistent `SECRET_KEY` configuration
and any additional application variables when adapting it:

```yaml
envs:
  - name: BIND_ADDR
    value: "0.0.0.0:8080"
  - name: DATABASE_URL
    value: "sqlite:///app/data/soika.db?mode=rwc"
  - name: BASE_URL
    value: "https://errors.example.com"
  - name: SERVICE_MANAGEMENT_SOURCE
    value: "env"
  - name: NOTIFICATIONS_ALERTMANAGER_ENABLED
    value: "true"
  - name: ALERTMANAGER_URL
    value: "http://alertmanager.monitoring:9093"
  - name: ALERTMANAGER_DEFAULT_INTERVAL_SECONDS
    value: "300"
  - name: ALERTMANAGER_DEFAULT_SEVERITY
    value: "warning" # Allowed: "critical", "warning", "info"; empty/unset defaults to critical.
  - name: PROJECT_MANAGEMENT_SOURCE
    value: "yaml"
  - name: TEAM_MANAGEMENT_SOURCE
    value: "yaml"
  - name: CONFIG_PATH
    value: "/etc/soika/config.yaml"
configFile:
  content:
    version: 1
    teams:
      - name: Platform
        members: []
    projects:
      - slug: api
        name: API
        team: Platform
        alertmanager_interval_seconds: 120
        alertmanager_severity: critical
      - slug: frontend
        name: Frontend
        team: Platform
        # Inherits 300 seconds and warning from env defaults.
```

Global defaults never belong in `configFile.content`. Remove the former root
`alertmanager_interval_seconds` field: it is now rejected by Soika. Only the
project field remains valid. Omit or set project overrides to null to inherit
the defaults. YAML management reconciles all projects and teams according to
orphan policies; include existing entities in your real configuration.

Changes to `envs` or `configFile.content` change the Deployment checksum and
trigger a rollout. When using `configFile.existingConfigMap`, restart the
Deployment after updating the external ConfigMap. Helm passes notification
placeholders such as `{{ project_slug }}` literally, without `tpl`.

Soika checks alert lifecycle changes every ten seconds and refreshes active
alerts at their effective interval. Resolved, muted or deleted issues close their
alerts on the next check. Enabling the channel does not send old issues.
**Send test alert** remains available in every management mode when the feature
flag is enabled; its fixed labels do not test project overrides.

SMTP and project webhooks retain `SMTP_*` and `WEBHOOKS_ENABLED`. Legacy
notification YAML compatibility is described in [Notifications](../docs/NOTIFICATIONS.md).
See [Soika Alertmanager setup and VMRule examples](../docs/ALERTMANAGER.md) for
connection settings, refresh intervals and project severity overrides.

## TLS Alertmanager и host aliases

`ALERTMANAGER_SKIP_TLS` в `envs` по умолчанию равен `"false"`. Значение `"true"`
отключает проверку TLS-сертификатов только для HTTPS-запросов к Alertmanager
во всех режимах управления, включая тестовые алерты и продление/закрытие.
Изменение применяется после rollout. Список `envs` заменяется целиком.

`hostAliases` задаёт статические записи в `/etc/hosts` pod’а (по умолчанию `[]`):

```yaml
hostAliases:
  - ip: "192.0.2.10"
    hostnames:
      - "alertmanager.internal"
```
