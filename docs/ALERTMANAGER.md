# Alertmanager: подключение и читаемые уведомления

Soika отправляет новые ошибки и регрессии напрямую в `/api/v2/alerts`.
Alertmanager группирует их и доставляет в настроенные каналы. Параллельно
vmalert может отправлять туда алерты по метрикам сервисов из VMRule:

```text
Sentry SDK → Soika ───────────────────────────→ Alertmanager → получатель → чат
Метрики сервиса → VictoriaMetrics → vmalert ──→ Alertmanager
                                      ↑ VMRule
```

VMRule нужны для мониторинга метрик, а не для обработки ошибок из Soika.
Метрики `hbb_*` ниже принадлежат happybirthday-bot; Soika их не экспортирует.
Проектный [webhook](WEBHOOKS.md) — отдельный канал с другим протоколом.

## Минимальная настройка Soika

1. Задайте окружение и перезапустите Soika:

   ```dotenv
   NOTIFICATIONS_ALERTMANAGER_ENABLED=true
   ALERTMANAGER_URL=http://alertmanager.monitoring:9093
   BASE_URL=https://errors.example.com
   SERVICE_MANAGEMENT_SOURCE=ui
   ```

2. Под Owner или Manager откройте **Admin → Notifications**, включите канал
   Alertmanager и сохраните настройки. URL — базовый, без `/api/v2/alerts`;
   пустое поле использует `ALERTMANAGER_URL`. `BASE_URL` задаёт публичные ссылки
   на ошибки. Поддерживаются HTTP(S) и префикс reverse proxy, но не HTTP-аутентификация.
3. Выполните **Send test alert**, затем создайте новую ошибку через SDK и
   проверьте сообщение в чате. Тест проверяет доставку, но использует фиксированный
   текст, поэтому свои шаблоны проверяйте через Preview и реальную новую ошибку.

В режиме `SERVICE_MANAGEMENT_SOURCE=env` канал и стандартные labels задаются
окружением; редактирование в UI недоступно. Проектные labels можно задать через
`CONFIG_PATH`. Настройка произвольных annotations через env не поддерживается;
для приведённых ниже шаблонов используйте UI или существующий legacy YAML mode.
Режимы управления, YAML-конфигурация и полный список переменных описаны в
[NOTIFICATIONS.md](NOTIFICATIONS.md); параметры развёртывания — в
[OPERATIONS.md](OPERATIONS.md#notifications).

## Короткий заголовок, подробности и действие

Стандартный `summary` Soika равен `{{ title }}`: туда может попасть длинный
текст HTTP-ошибки с URL и JSON. Получатель, который печатает только summary,
покажет именно эту строку. Для человека полезнее:

- `summary`: сервис и коротко описанная проблема;
- `description`: влияние, контекст и что проверить; сырой текст ошибки — здесь;
- `issue_url` / `runbook_url`: ссылка на ошибку или инструкцию;
- labels: стабильные поля для маршрутизации (`project`, `job`, `cluster`, `severity`, `chat`).

Например, в **Admin → Notifications** замените annotations этим JSON:

```json
{
  "summary": "{{ project_name }}: ошибка приложения",
  "description": "Ошибка: {{ title }}\nМесто: {{ culprit }}\nОкружение: {{ environment }}\nРелиз: {{ release }}\nСобытий: {{ event_count }}\nПоследнее событие: {{ last_seen }}\nОткройте issue и проверьте последнее событие и стек вызовов.",
  "issue_url": "{{ issue_url }}",
  "trigger": "{{ trigger }}"
}
```

Пример labels для общей схемы маршрутизации:

```json
{
  "alertname": "SoikaIssue",
  "project": "{{ project_slug }}",
  "job": "{{ project_slug }}",
  "severity": "{{ level }}",
  "chat": "service-general-status"
}
```

Soika всегда добавляет `issue_id` и `project_id`. Её уровни — Sentry levels
(`fatal`, `error`, `warning`, `info`, `debug`), а у VMRule ниже — `critical` и
`warning`. Учтите оба набора в маршрутах Alertmanager или задайте постоянную
severity для проекта через project labels. Синтаксис Soika — только подстановка
`{{ variable }}`: условия, `printf` и `$labels` здесь недоступны. Шаблоны общие
для всех проектов; автоматически превратить произвольный лог в объяснение
«KeyHub отклоняет подпись» они не могут. Такие точные тексты задаются в правилах
метрик, как ниже.

Не помещайте title, счётчик событий и время в labels: изменение label создаёт
новую идентичность алерта. Они должны оставаться annotations.

## Сообщение в чате и статусы

Получатель должен выводить description и ссылку, а статус брать у каждого
алерта. Не добавляйте постоянный `✅` к summary. В шаблоне Alertmanager
`.Status` всей группы означает firing, если хотя бы один её алерт firing;
внутри `range .Alerts` это статус конкретного алерта.
[Справочник шаблонов Alertmanager](https://prometheus.io/docs/alerting/latest/notifications/).

Пример текстового шаблона `/etc/alertmanager/templates/readable.tmpl`:

```gotemplate
{{ define "readable.message" }}
{{ range .Alerts }}
{{ if eq .Status "firing" }}🔥 Активен{{ else }}✅ Закрыт{{ end }} [{{ .Labels.severity }}]
{{ .Annotations.summary }}
{{ with .Labels.cluster }}Кластер: {{ . }}{{ end }}
{{ with .Labels.instance }}Экземпляр: {{ . }}{{ end }}
{{ with .Annotations.description }}{{ . }}{{ end }}
{{ if .Annotations.issue_url }}Issue: {{ .Annotations.issue_url }}{{ else if .GeneratorURL }}Источник: {{ .GeneratorURL }}{{ end }}
{{ with .Annotations.runbook_url }}Инструкция: {{ . }}{{ end }}
{{ end }}
{{ end }}
```

Подключите файл через `templates` и вызовите `{{ template "readable.message" . }}`
в текстовом поле своего receiver. Например, для Slack:

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
    # Добавьте ваш канал для алертов без label chat.
  - name: service-general-status
    slack_configs:
      - api_url_file: /etc/alertmanager/secrets/slack-webhook-url
        channel: '#service-general-status'
        send_resolved: true
        title: 'Уведомления сервиса'
        text: '{{ template "readable.message" . }}'
```

Это пример конфигурации обычного Alertmanager; объедините его со своими
routes/receivers. `fallback` здесь не доставляет сообщения, пока вы не добавите
интеграцию. `chat` — условный label, он сам не выбирает чат без соответствующего
маршрута. Настройку receiver сверяйте с
[конфигурацией Alertmanager](https://prometheus.io/docs/alerting/latest/configuration/).

При webhook-доставке в Пачку или собственный шлюз сообщение строит сам шлюз
по `alerts[].status`, `alerts[].annotations` и `alerts[].generatorURL`.
Файлы `.tmpl` Alertmanager не меняют JSON webhook. Измените шаблон шлюза,
если он сейчас выводит только summary или всегда ставит зелёную галочку.
`commonAnnotations` содержит только общие annotations: при группировке разных
ошибок их descriptions могут отсутствовать там, поэтому обходите `alerts[]`.

Пример ожидаемого сообщения по метрикам:

```text
🔥 Активен [critical]
happybirthday-bot: кэш дней рождения не наполнен
Кластер: k8s-internal
Экземпляр: 10.0.0.15:8080
Ни одна выгрузка из KeyHub не завершилась успешно за 30 минут.
Рассылка будет пропущена. Проверьте KEYHUB_SECRET_KEY и доступность KeyHub.
```

Для ошибок Soika «Закрыт» означает завершение алерта, а не подтверждённое
восстановление сервиса: resolve, mute, удаление и отключение канала тоже закрывают
алерты. Они обновляются каждые 60 секунд и истекают через пять минут без
обновления. Включение канала не отправляет старые issues. Подробнее о повторных
попытках и смене шаблонов — в [жизненном цикле уведомлений](NOTIFICATIONS.md#alertmanager).

## VMRule: happybirthday-bot

Пример основан на метриках happybirthday-bot. До применения проверьте имена
метрик, scrape label `job` и значения порогов в своём datasource. Здесь
`job="k8s-internal-happybirthday-bot"` выбирает исходные ряды, а rule label
`job: happybirthday-bot` задаёт имя сервиса для уведомлений. Если datasource
содержит несколько кластеров с одинаковым job, добавьте их исходный cluster
label в каждый селектор и в ключи агрегации/сопоставления.

VMRule должен попадать под `ruleSelector` и `ruleNamespaceSelector` вашего
VMAlert; добавьте требуемые metadata labels. VMAlert должен читать нужный
datasource и отправлять уведомления в тот же Alertmanager.
[Документация VMRule](https://docs.victoriametrics.com/operator/resources/vmrule/),
[выбор правил VMAlert](https://docs.victoriametrics.com/operator/resources/vmalert/).

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
            summary: 'happybirthday-bot: под не отвечает на скрейп'
            description: |-
              Экземпляр {{ $labels.instance }} недоступен более 10 минут.
              Проверьте состояние пода, порт, scrape-аннотации и /healthz.

        - alert: HBBCacheNeverPopulated
          expr: hbb_cache_ready{job="k8s-internal-happybirthday-bot"} == 0
          for: 30m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: кэш дней рождения не наполнен'
            description: |-
              После старта {{ $labels.instance }} ни одна выгрузка из KeyHub не завершилась успешно.
              Рассылка будет пропущена с исходом not_ready.
              Проверьте KEYHUB_SECRET_KEY, KEYHUB_BASE_URL и доступность KeyHub.

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
            summary: 'happybirthday-bot: данные KeyHub устарели'
            description: |-
              Последняя успешная выгрузка на {{ $labels.instance }} была более 6 часов назад.
              Новые и уволенные сотрудники не учитываются. Проверьте ошибки выгрузки и доступность KeyHub.

        - alert: HBBKeyHubFetchErrors
          expr: increase(hbb_keyhub_fetch_total{job="k8s-internal-happybirthday-bot",result="error"}[2h]) >= 2
          for: 5m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: повторяющиеся ошибки выгрузки из KeyHub'
            description: |-
              За 2 часа выгрузка на {{ $labels.instance }} падала больше одного раза.
              Прежний кэш сохраняется. Проверьте логи запросов к KeyHub.

        - alert: HBBKeyHubUnauthorized
          expr: increase(hbb_keyhub_http_requests_total{job="k8s-internal-happybirthday-bot",code=~"401|403"}[1h]) > 0
          for: 5m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: KeyHub отклоняет запросы ({{ $labels.code }})'
            description: |-
              KeyHub отвечает {{ $labels.code }} на {{ $labels.instance }}.
              Проверьте KEYHUB_SECRET_KEY, KEYHUB_APP_ID и правила подписи запроса.

        - alert: HBBKeyHubHTTPErrors
          expr: increase(hbb_keyhub_http_requests_total{job="k8s-internal-happybirthday-bot",code!~"200|401|403"}[1h]) > 0
          for: 15m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: ошибки HTTP при обращении к KeyHub ({{ $labels.code }})'
            description: |-
              За час были ошибки с кодом {{ $labels.code }} на {{ $labels.instance }}.
              code="error" означает сетевую ошибку или таймаут. Проверьте KeyHub и сетевые политики.

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
            summary: 'happybirthday-bot: выгрузка из KeyHub замедлилась'
            description: |-
              Средняя полная выгрузка за 6 часов на {{ $labels.instance }} превышает 30 секунд.
              Возможен срыв обновления при лимите цикла 2 минуты. Проверьте задержки KeyHub.

        - alert: HBBSendFailed
          expr: increase(hbb_pachca_send_total{job="k8s-internal-happybirthday-bot",result="error"}[15m]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: не удалось отправить поздравление в Пачку'
            description: |-
              Отправка на {{ $labels.instance }} завершилась ошибкой, маркер дедупа не выставлен.
              Повтор возможен при рестарте или в следующий SEND_AT; сегодняшнее поздравление может быть потеряно.
              Проверьте ответ Пачки, токен и доступ бота к каналу.

        - alert: HBBPachcaAuthError
          expr: increase(hbb_pachca_send_errors_total{job="k8s-internal-happybirthday-bot",kind="auth"}[24h]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: ошибка авторизации в Пачке'
            description: |-
              Пачка вернула ошибку авторизации.
              Проверьте PACHCA_TOKEN и доступ бота к PACHCA_CHANNEL_ID.

        - alert: HBBRenderError
          expr: increase(hbb_congratulate_total{job="k8s-internal-happybirthday-bot",outcome="render_error"}[1h]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: critical
            chat: service-general-status
          annotations:
            summary: 'happybirthday-bot: не удалось отрендерить поздравление'
            description: |-
              Именинники есть, но сообщение не построено.
              Проверьте phrases.yaml в ConfigMap и ошибку рендера в логах.

        - alert: HBBSendSkippedNotReady
          expr: increase(hbb_congratulate_total{job="k8s-internal-happybirthday-bot",outcome="not_ready"}[1h]) > 0
          for: 5m
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: рассылка пропущена — кэш не готов'
            description: |-
              В момент рассылки кэш на {{ $labels.instance }} был пуст, сообщение не отправлено.
              Проверьте ошибки выгрузки из KeyHub.

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
            summary: 'happybirthday-bot: суточный цикл поздравлений не отрабатывал'
            description: |-
              За 26 часов нет ни одного исхода рассылки на {{ $labels.instance }}.
              Под работает больше 26 часов. Проверьте планировщик, логи и SEND_AT/APP_TZ.

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
            summary: 'happybirthday-bot: кэш дней рождения пуст'
            description: |-
              KeyHub отвечает успешно, но подходящих сотрудников нет ({{ $labels.instance }}).
              Проверьте KEYHUB_BIRTHDAY_FIELD, KEYHUB_BIRTHDAY_FORMAT и KEYHUB_ACTIVE_ONLY.
              Причины пропуска записей доступны в DEBUG-логах.

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
            summary: 'happybirthday-bot: в кэше стало заметно меньше сотрудников'
            description: |-
              Число сотрудников на {{ $labels.instance }} упало более чем на 20% за сутки (сейчас {{ $value | printf "%.0f" }}).
              Проверьте массовую блокировку учёток, фильтры и полноту выгрузки.

        - alert: HBBStateWriteErrors
          expr: increase(hbb_state_write_errors_total{job="k8s-internal-happybirthday-bot"}[1h]) > 0
          labels:
            job: happybirthday-bot
            cluster: k8s-internal
            severity: warning
          annotations:
            summary: 'happybirthday-bot: не удалось сохранить маркер отправки'
            description: |-
              Поздравление отправлено, но состояние в STATE_DIR не записано ({{ $labels.instance }}).
              При рестарте возможен дубль. Проверьте заполненность, права и монтирование PVC.
```

В отличие от исходного примера, селекторы ограничены job сервиса, cluster
передаётся отдельным label, а HTTP warning исключает 401/403, для которых есть
отдельный critical. Внутри VMRule разрешён синтаксис vmalert
`{{ $labels.instance }}` / `{{ $value | printf "%.0f" }}`; он не подходит
для шаблонов Soika.

`increase(...[1h]) > 0` означает ошибку в окне истории, а не текущую поломку:
алерт может оставаться активным до выхода ошибки из окна. `for` требует,
чтобы выражение оставалось истинным указанное время. Условия с `offset 24h`
требуют истории. `up == 0` обнаруживает неуспешный скрейп существующей цели;
для исчезнувшей из discovery цели требуется отдельное правило, например
`absent(up{job="k8s-internal-happybirthday-bot"})` с подходящим `for`.
Правило суточного цикла предполагает заранее созданные ряды счётчика;
отсутствующие метрики требуют отдельного контроля.

Перед применением проверьте выражения в datasource и шаблоны средствами своей
версии vmalert. После применения проверьте загрузку правил в VMAlert, firing
в Alertmanager, текст в чате и отдельное resolved-сообщение. При публикации YAML
через Helm экранируйте внутренние `{{ ... }}`, если файл проходит через `tpl`.
