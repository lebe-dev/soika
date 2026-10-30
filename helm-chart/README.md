# Soika Helm chart

This chart deploys the single-binary Soika image with SQLite or PostgreSQL 15+.
It uses one replica and a `Recreate` deployment strategy: the retention scheduler,
ingestion rate limiter, and login lockout are per process. The application runs
the selected database's migrations at startup; no migration Job is needed.
Chart version: `0.3.1`. Default application image: `tinyops/soika:0.10.0`.

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
the root `.env.example`.
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
claim, по умолчанию `groups`. Без OAuth новые пользователи не смогут войти в
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

В **Admin → Notifications** находятся Email/SMTP, Webhooks проектов и Alertmanager.
Для доступности Alertmanager установите `NOTIFICATIONS_ALERTMANAGER_ENABLED=true`
в `envs` (по умолчанию `false`). При выключенном флаге UI коротко сообщает об
отключении в конфигурации; отправка, продление и закрытие алертов заблокированы.
Открытые алерты истекут по TTL; записи в БД сохранятся до включения флага.
По умолчанию `SERVICE_MANAGEMENT_SOURCE=ui`: URL Alertmanager и шаблоны
labels/annotations сохраняются через UI в БД. `ALERTMANAGER_URL` в `envs` задаёт
адрес по умолчанию: пустой URL в UI/YAML использует его, явно указанный URL
имеет приоритет. Изменение переменной окружения применяется после rollout.
В режиме `ui` после установки глобального флага включите канал кнопкой
**Enable Alertmanager** и сохраните изменения: флаг разрешает интеграцию,
но не меняет сохранённое состояние канала. При включённом канале доступна
отправка тестового алерта с фиксированным текстом на языке интерфейса.
В режиме `env` настройки и тестовая отправка заблокированы в UI и API;
форма помечена «Настраивается через переменные окружения». Имя организации
берётся из `ORGANIZATION_NAME` и недоступно для изменения даже администраторам.
При старте настройки уведомлений в БД заменяются значениями окружения:
`NOTIFICATIONS_ALERTMANAGER_ENABLED` включает канал, `ALERTMANAGER_URL` задаёт
адрес, шаблоны используются встроенные. Изменения применяются после rollout.
Значения `yaml` и `yml` для `SERVICE_MANAGEMENT_SOURCE` больше не поддерживаются.

Пример режима `env` (список `envs` заменяется целиком):

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
  - name: ORGANIZATION_NAME
    value: "My organization"
  - name: NOTIFICATIONS_ALERTMANAGER_ENABLED
    value: "true"
  - name: ALERTMANAGER_URL
    value: "http://alertmanager.monitoring:9093"
```

Секция `configFile.content.notifications` в режимах `ui` и `env` игнорируется.
Для совместимости старый `NOTIFICATION_MANAGEMENT_SOURCE=yaml` работает,
если `SERVICE_MANAGEMENT_SOURCE` отсутствует; только этот режим использует
секцию уведомлений YAML.

Helm передаёт `{{ variable }}` буквально, без `tpl`. Изменение содержимого
`configFile` меняет checksum и вызывает rollout; для `existingConfigMap`
после обновления внешнего ConfigMap перезапустите Deployment самостоятельно.
Режим уведомлений не зависит от источника проектов и команд. В YAML-режиме
настройки в UI доступны только для чтения. По умолчанию канал выключен.

Soika отправляет алерты через `/api/v2/alerts`, продлевает открытые каждую минуту
и закрывает их при resolve/mute/удалении. Старые ошибки при включении не отправляются.
SMTP и Webhooks сохраняют настройки `SMTP_*` и `WEBHOOKS_ENABLED`.
Список переменных и поведение доставки описаны в [Notifications](../docs/NOTIFICATIONS.md).

В режиме `SERVICE_MANAGEMENT_SOURCE=env` все общие настройки сервиса недоступны
для изменения через UI/API: имя организации (`ORGANIZATION_NAME`), регистрация
(`ALLOW_SIGNUP`), команды новых пользователей (`DEFAULT_TEAM_IDS`, UUID через запятую;
по умолчанию пусто) и уведомления. Команды должны существовать к моменту запуска.
Mute, управление пользователями, проектами и командами сохраняют текущие правила доступа.
