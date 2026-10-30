# Operator guide

This document contains setup and operating details for soika. For a short
project overview, see the [README](../README.md); for development instructions,
see [DEV.md](../DEV.md).

**soika** is a lightweight, Sentry-compatible error tracking system written in
Rust. It is designed from the ground up for minimal resource consumption and
high throughput.

**Status:** work in progress.

- **Drop-in ingestion** for the modern Sentry SDK envelope protocol.
- Captures, groups, and displays **errors/exceptions and messages** with full
  **stacktraces** (Go, Rust, Svelte/JS, and any Sentry-compatible SDK).
- A complete, self-contained web UI for triaging issues.
- Ships as a **single binary** with the frontend embedded, plus a minimal
  Alpine container image — no Redis, no broker, no separate worker.

## Application logs

Logs use `tracing` and are written to stdout. `RUST_LOG` defaults to `info`;
set `RUST_LOG=info,soika=debug` temporarily for application diagnostics.
Avoid enabling dependency-wide DEBUG/TRACE in production: dependency logs may
include SQL bind values or external request details outside the application's
redaction rules.

At INFO, lifecycle events include the application version and readiness;
each HTTP request emits `request received` and `request completed` with a
server-generated `request_id`, HTTP `method`, route template (`route`), numeric
`status`, and `duration_ms`. Authenticated request logs also carry `user_id`
after authentication; project authorization adds `project_id`. Search by
`request_id` to follow an individual request. URLs, query strings, headers,
bodies, invite tokens, and recipient email addresses are omitted. SPA and
unmatched paths use `<fallback>` instead of the raw URL.

Sign-ins, access changes, issue/project updates and notification delivery
outcomes are logged with entity IDs. Scheduler sweeps have a separate `job_id`
and report their duration and the outcome of retention and mute expiry even
when no rows change. Per-project pruning and retry attempts are DEBUG details.
Transient database contention and handled delivery failures use WARN; terminal
server faults use ERROR, at the boundary handling the failure. When self-reporting
is enabled, ERROR events also go to Sentry. OAuth errors retain failure categories
instead of provider descriptions or token/claim values; webhook failures omit
the destination URL, and SMTP failures retain status and transport categories
instead of relay response text.

## Roles & access

soika has two independent role axes plus a derived per-project access level.

### Instance roles

Every user has one instance-wide role (`Owner | Manager | Member`):

- **Owner** — full control. Can do everything a Manager can, plus
  grant/revoke the `Owner` role and perform destructive/critical instance
  operations.
- **Manager** — runs everything operational: sees and administers **every**
  team and project, creates/deletes teams, manages team composition and
  team roles, invites users, approves pending sign-ups, deletes users and
  changes instance roles (but cannot grant/revoke `Owner`).
- **Member** — no instance-wide powers. Access is limited to the projects of
  the teams the user belongs to (see team roles below).

The first user, created on the `/setup` page on first run, is an `Owner`. Users
created by self-registration or auto-provisioned via SSO start as `Member`.
Set `ADMIN_ENABLED=false` to hide and disable first-run setup; then use
`ADMIN_USERS` to grant Owner permissions to an existing account. If OAuth is
also disabled, the service logs a startup warning so the operator can verify
that an admin account can authenticate.

### Administrators from environment configuration

Set `ADMIN_USERS` to a comma-separated list of **full email addresses** to give
those accounts the same effective `Owner` permissions as the setup administrator:

```env
ADMIN_USERS=alice@example.com,bob@example.com
```

Matching ignores ASCII letter case and surrounding spaces; empty items and
duplicates are ignored. Usernames, display names, domains and wildcards are not
accepted. The default is an empty list. Invalid entries prevent startup.

The grant applies to new and existing accounts, across password sign-in,
OAuth/OIDC (including GitHub), invite acceptance and passkeys. Normal authentication,
email verification, allowed-domain, sign-up and account-approval gates still apply.
Listing an address does not create an account or approve a pending one. Listed addresses cannot register locally, including through invites, because registration does not verify mailbox ownership. Create and verify local accounts before adding them to `ADMIN_USERS`, or provision them through verified SSO. Existing self-registered local emails are unverified claims; inspect their ownership before granting privileges. A non-empty `ADMIN_USERS` disables anonymous first-run setup, preventing a second bootstrap Owner outside the configured identities. Only an Owner may approve a pending effective Owner account. Listed
local accounts may use their password even when SSO is enabled; listed accounts
may use SSO even when their stored role is `Owner`. Unlisted stored Owners retain
the password-only SSO protection.

`ADMIN_USERS` overrides permissions without changing the role stored in the
database. The current-user profile and **Admin → Users** report the effective Owner role.
Listed accounts have a fixed role: neither the UI nor the API allows changing it
while the environment grant remains active. Only an Owner can delete a listed account.
This also applies when projects and teams are managed through YAML; `ADMIN_USERS`
is still configured through the environment.
After changing the list, restart the service: removing an address immediately
restores its stored role on subsequent requests, including existing sessions.
Roles assigned through the UI are preserved. Last-Owner protection continues to
count stored Owners, ensuring that the setup/recovery account remains available
independently of this configuration.

### Authentication overview in the admin UI

**Admin → Authentication** shows the active authentication policy as read-only
information: SSO provider, allowed email domains and new-account approval,
password access (all local accounts, local Owners only, or disabled), passkey
availability and relying-party settings, effective public registration, initial
administrator setup and password brute-force protection. YAML team management
restrictions are shown when enabled. SSO credentials and secrets are never
included. Change the server configuration and restart to update these settings.

### Team membership and team roles

**Project access is team-only and closed:** a user can see a project **only**
if they are a member of the team that owns it. There are no direct
user-to-project memberships and no "open membership" — being a Member of the
instance grants nothing on its own. Each team membership carries a team role:

- **Admin** — manages the team: composition and team roles, project settings,
  and creating/deleting projects within the team.
- **Contributor** — read and triage the team's projects (view issues/events,
  resolve/mute issues, mute-rules), but no project settings or team management.

Instance `Owner`/`Manager` implicitly act as Admin on every team and project.

### Default teams for new users

An instance Owner or Manager can select teams in **Admin → Users → Teams for
new users** and save the selection. The initial selection is empty: new users
join no teams automatically until an administrator configures it. Clear all
checkboxes and save to disable automatic membership again.

Only newly created accounts join the selected teams, with the **Contributor**
role. This applies to password registration, new accounts created from invites,
and first SSO sign-in. An invitation's explicit team role still takes precedence
for its target team, including an Admin invitation to a default team. SSO
accounts requiring approval receive memberships at creation but cannot access
the service until approved.

Saving or changing defaults never adds, removes, or changes memberships of
existing users. Signing in again or approving an existing account does not
apply defaults; manually removed memberships stay removed. Deleting a selected
team removes it from the defaults automatically. Account creation and its
default memberships are committed together.

### Effective per-project access

A caller's effective access to a given project resolves as:

1. instance `Owner` or `Manager` ⇒ **Admin** on every project;
2. otherwise the caller's role on the **owning team**: team `Admin` ⇒ project
   **Admin**, team `Contributor` ⇒ project **Member**;
3. otherwise **no access** (closed membership).

Because project access derives entirely from the owning team, **moving a project
to another team reassigns who can see it** — the previous team loses access and
the new team gains it. This is an instance-management action: only an instance
`Owner`/`Manager` can move a project (from the project's **Settings → Owning
team**), and they may move it to any team.

### Permission matrix

| Action | Member / Contributor | Team Admin | Manager | Owner |
|---|---|---|---|---|
| View issues/events of own teams | ✔ | ✔ | ✔ (all) | ✔ (all) |
| Resolve / mute issue, mute-rules | ✔ | ✔ | ✔ | ✔ |
| Project settings (retention, webhook, DSN, mute, rename) | – | ✔ | ✔ | ✔ |
| Create / delete project in a team | – | ✔ | ✔ | ✔ |
| Move a project to another team | – | – | ✔ | ✔ |
| Manage team composition + team roles | – | ✔ (own) | ✔ (all) | ✔ (all) |
| Create / delete a team | – | – | ✔ | ✔ |
| Invites, approve pending, delete users, set instance roles | – | – | ✔ | ✔ |
| Grant/revoke **Owner**, destructive instance ops | – | – | – | ✔ |

### Last-Owner protection

The instance must always retain at least one `Owner`: the last remaining Owner
cannot be demoted or deleted. Likewise a team always keeps at least one team
`Admin` — the last team Admin cannot be demoted or removed.

### Invites

Invites are **team-scoped**: an invite targets a team and carries the team role
(`Admin` or `Contributor`) the invitee receives on acceptance. Accepting an
invite makes the user a member of that team. Issuing or revoking a team's
invites is allowed for an instance `Owner`/`Manager` or a team `Admin` of that
team.

## Notifications

**Admin → Notifications** also configures the Alertmanager channel and its label/
annotation templates. Choose UI or YAML ownership with
`NOTIFICATION_MANAGEMENT_SOURCE=ui|yaml` (default `ui`), independently of projects
and teams. See [notification configuration, lifecycle and template variables](NOTIFICATIONS.md).

Outgoing email is disabled by default. Set `SMTP_ENABLED=true` and configure
`SMTP_HOST` (plus any required SMTP credentials) to enable email notifications,
invite delivery and the admin test-email action. The Admin → Notifications → Email / SMTP card shows
whether delivery is enabled and configured.

Project webhooks are also disabled by default. Set `WEBHOOKS_ENABLED=true` to
send project issue and regression notifications to configured webhook URLs.
When disabled, the webhook URL field in project settings is visually disabled
and the service does not send webhook requests. These switches are read at
startup; restart the service after changing them.

Webhook payloads use the Alertmanager v4 JSON format since 0.10.0. See
[Outgoing webhooks](WEBHOOKS.md) for setup, payload fields and delivery behavior.

## SSO via OIDC

soika can delegate sign-in to an external OpenID Connect (OIDC) provider such as
GitLab, Keycloak, Authentik, Google, or Okta. SSO is **disabled by default**
(`OAUTH_ENABLED=false`). Email/password login is enabled by default
(`EMAIL_LOGIN_ENABLED=true`).

### 1. Register an OAuth application at your provider

Create an application/client in your identity provider and note its **Client ID**
and **Client Secret**. Configure it with:

- **Redirect URI:** `{BASE_URL}/auth/oidc/callback`
  (e.g. `https://errors.example.com/auth/oidc/callback`). It must match
  `OAUTH_REDIRECT_URL` (or `{BASE_URL}/auth/oidc/callback` if that is left
  unset).
- **Scopes:** `openid email profile`.

### 2. Configure soika

SSO accounts cannot edit their display name by default, including through the
profile API (`403 Forbidden`). Set `SSO_ALLOW_DISPLAY_NAME_CHANGE=true` and restart
to allow it. This applies to both OIDC and GitHub SSO accounts, regardless of
instance role, and also applies when SSO sign-in is disabled. Local accounts can
still edit their name; other profile preferences remain editable for everyone.

Set the following environment variables (see `.env.example` for the full list and
defaults):

```bash
OAUTH_ENABLED=true
OAUTH_ISSUER_URL=https://gitlab.com
OAUTH_CLIENT_ID=<from your provider>
OAUTH_CLIENT_SECRET=<from your provider>
# Optional overrides:
# OAUTH_REDIRECT_URL=https://errors.example.com/auth/oidc/callback
# OAUTH_SCOPES=openid email profile
# OAUTH_PROVIDER_NAME=GitLab
# OAUTH_ALLOWED_EMAIL_DOMAINS=example.com,itkey.com
```

Discovery uses `{OAUTH_ISSUER_URL}/.well-known/openid-configuration`. When
`OAUTH_ENABLED=true`, the three required variables (`OAUTH_ISSUER_URL`,
`OAUTH_CLIENT_ID`, `OAUTH_CLIENT_SECRET`) must be present or the server fails
fast at startup.

#### GitLab example

For GitLab.com, register the application under **User Settings → Applications**,
grant the `openid`, `email`, and `profile` scopes, set the redirect URI to
`{BASE_URL}/auth/oidc/callback`, and configure:

```bash
OAUTH_ENABLED=true
OAUTH_ISSUER_URL=https://gitlab.com
OAUTH_CLIENT_ID=<Application ID>
OAUTH_CLIENT_SECRET=<Secret>
OAUTH_SCOPES=openid email profile
```

A self-managed GitLab instance works the same way; point `OAUTH_ISSUER_URL` at
your instance URL (e.g. `https://gitlab.example.com`).

### 3. HTTPS requirement in production

OIDC requires HTTPS in production. soika marks session and OAuth state cookies as
`Secure` only when `BASE_URL` starts with `https://`, so set `BASE_URL` to your
externally reachable `https://` URL when deploying behind a proxy or in
production.

### Coexistence with password login

When SSO is enabled, OAuth replaces the password login for all regular users:

- The login screen shows a **"Sign in with {OAUTH_PROVIDER_NAME}"** button
  with the Owner password form collapsed under **Administrator sign-in**.
  Self-registration is disabled.
- A user is matched by the verified email from the provider; if no account
  exists, a new user is auto-provisioned with the `Member` instance role and no
  team membership unless default teams or declarative grants assign it.
  YAML-managed team grants apply immediately and can approve the account.
- Use `OAUTH_ALLOWED_EMAIL_DOMAINS` to restrict which email domains may
  auto-provision, since auto-creation trusts the provider's email claim.
- Stored instance Owners and local accounts listed in `ADMIN_USERS` keep
  password login while `EMAIL_LOGIN_ENABLED=true`, so the instance remains
  accessible if SSO is misconfigured.

### Disable email/password sign-in

Set `EMAIL_LOGIN_ENABLED=false` and restart soika to hide the email/password form
and reject `POST /auth/login` with `403` for every account, including Owners and
accounts listed in `ADMIN_USERS`. The public `/auth/config` response exposes this
setting as `password_login_enabled`. The configured SSO or passkey sign-in
method remains available; they cannot be enabled together.

This also disables public password registration and anonymous password-based
invite acceptance, preventing alternative password sign-in paths. Invitees must
sign in via SSO or a passkey first, then accept the invite. Existing sessions are unaffected by the email-login flag alone; enabling SSO
also invalidates local non-Owner sessions. First-run setup is unaffected. Configure SSO or passkeys before
disabling email login; if no other sign-in method is available, restore
`EMAIL_LOGIN_ENABLED=true` and restart. The default is `true`.

## Passkeys (WebAuthn)

Passkeys let a user sign in with Touch ID / Face ID / Windows Hello or a
security key instead of a password. The feature is **off by default**
(`PASSKEY_ENABLED=false`); enabling it changes nothing for accounts that do not
register a key.

### 1. Enable it

Everything is derived from `BASE_URL`, so a standard deployment needs one line:

```env
BASE_URL=https://errors.example.com
PASSKEY_ENABLED=true
```

That binds credentials to the relying party id `errors.example.com` and accepts
assertions from the origin `https://errors.example.com`. Override
`PASSKEY_RP_ID`, `PASSKEY_RP_NAME`, `PASSKEY_RP_ORIGIN`,
`PASSKEY_EXTRA_ORIGINS`, `PASSKEY_ALLOW_SUBDOMAINS`, `PASSKEY_TIMEOUT_SECONDS`,
`PASSKEY_CHALLENGE_TTL_SECONDS` or `PASSKEY_MAX_PER_USER` only when the defaults
do not fit — `.env.example` documents each one.

> **`PASSKEY_RP_ID` is permanent.** Credentials are cryptographically bound to
> it, so changing it (or moving the instance to another domain) invalidates
> every registered passkey and users must register again.

WebAuthn only works in a *secure context*: `https://`, or `http://localhost` for
local development. Behind a proxy, set `BASE_URL` to the externally reachable
`https://` URL.

### 2. Register a key

A signed-in user opens **Profile → Passkeys**, names the key, and confirms the
browser prompt. Keys can be removed there at any time; each user may register up
to `PASSKEY_MAX_PER_USER` of them.

### 3. Sign in

The login page shows **"Sign in with a passkey"** whenever the feature is on and
the browser supports WebAuthn. No email is typed: the browser offers the
credentials it holds for this site, and soika resolves the account from the
credential itself — so the sign-in form leaks nothing about which accounts
exist.

Passkey sign-in shares the brute-force guard with password login (keyed per
client IP) and honours the same account gates: a `pending` account still has to
be approved by an instance admin before it receives a session.

Passkeys coexist with password login and require SSO to be disabled. Enabling
`PASSKEY_ENABLED` together with `OAUTH_ENABLED` is a startup error. An account
with no registered key is unaffected. Deleting a user deletes its credentials. Ceremony state is stored in the database and consumed atomically once; the browser cookie carries only a random handle. This works across replicas sharing the database. Unfinished ceremonies from before the upgrade must be restarted. Anonymous login options are limited to 300 requests per minute per instance, independently of failed assertions.

## Interface language

The interface supports **English** and **Russian**, using `svelte-intl-precompile`.
English is the fallback. On an anonymous visit and the first authenticated visit
(including after signup, setup, SSO or passkey sign-in), the SPA checks the browser's
primary language: `ru` and regional variants such as `ru-RU` select Russian;
every other language selects English.

The initial authenticated choice is saved in SQLite. **Profile → Language** lets
each user change it; that saved preference takes precedence on subsequent visits,
including from other browsers and devices. Existing accounts detect their language
on their next authenticated visit after upgrading. Dates and counts follow the
interface language; the configured display timezone still applies.

The profile API accepts `locale: "en" | "ru"` for an explicit choice and
`initial_locale: "en" | "ru"` to initialize an unset preference atomically without
overwriting a saved choice. `locale: null` in a user response means detection has
not yet been saved. Email templates, SDK payloads, source code and unknown backend
diagnostic messages retain their original language.

Translation catalogs live in `frontend/locales/en.json` and `ru.json`; keep their
keys in sync and use ICU plural messages for counts. Run `just dist` after changing
the frontend so the single binary embeds the updated translations.

## Database backends

soika stores its data in **SQLite** (the default) or **PostgreSQL 15 or
newer**. The scheme of `DATABASE_URL` picks the backend:

| `DATABASE_URL` | Backend |
| --- | --- |
| `sqlite://data/soika.db` | SQLite file, created if missing. |
| `postgres://user:password@host:5432/soika` (or `postgresql://`) | PostgreSQL. The database must exist; add `?sslmode=require` for TLS. |

Both backends run their schema migrations automatically at startup, so a fresh
PostgreSQL database only needs to be created and reachable. The two backends
hold the same data but there is no built-in tool to copy an existing SQLite
database into PostgreSQL yet; switching backends starts from an empty instance.

On PostgreSQL:

- `DB_MAX_CONNECTIONS` and `DB_ACQUIRE_TIMEOUT_MS` size the connection pool.
  Keep `DB_MAX_CONNECTIONS` well below the server's `max_connections`.
- Writes that hit a deadlock, a serialization failure or a lock timeout are
  retried with the same `DB_WRITE_*` budget; an exhausted budget gives the same
  `429 Retry-After` (ingestion) / `503` (API) as on SQLite.
- Retention still deletes in `DB_DELETE_BATCH` chunks to keep transactions small.
- `DB_BUSY_TIMEOUT_MS` and `DB_SYNCHRONOUS` are SQLite-only and ignored.

An example Compose stack running soika against PostgreSQL 15 is in
`docker-compose-postgres.yml`.

## Database & write concurrency (SQLite)

soika stores everything in one SQLite file. SQLite allows **many readers but
exactly one writer at a time**, so on a busy instance event ingestion competes
for the write lock with the retention sweep and with the UI's own writes. soika
handles that contention itself; the knobs below only exist for tuning it (all are
optional, and the defaults are the recommended values).

How it works, so the settings make sense:

- The database runs in **WAL** mode, so reading the UI never blocks ingestion.
- Every write transaction is opened as `BEGIN IMMEDIATE`, i.e. it claims the
  write lock up front. This is what makes `DB_BUSY_TIMEOUT_MS` effective: a
  transaction that instead started as a reader and later tried to *become* a
  writer would be refused instantly (`database is locked`) with no waiting
  possible.
- A write refused anyway is **retried** with exponential backoff and jitter
  (`DB_WRITE_*`).
- Bulk retention deletes run in **batches** (`DB_DELETE_BATCH`), releasing the
  lock between batches, so a sweep over a large backlog cannot starve ingestion.
- If a write is *still* refused after the whole retry budget, ingestion answers
  **`429` with `Retry-After: DB_BUSY_RETRY_AFTER_SECS`** instead of `500`. Sentry
  SDKs treat `429` as backpressure and re-send the event later, so events are not
  lost; the internal JSON API answers `503` in the same situation.

| Variable | Default | Meaning |
| --- | --- | --- |
| `DATABASE_URL` | `sqlite://soika.db` | Database file; created if missing. A `postgres://` URL selects PostgreSQL (see above). |
| `DB_MAX_CONNECTIONS` | `8` | Connection pool size. |
| `DB_BUSY_TIMEOUT_MS` | `5000` | How long SQLite waits for the write lock before reporting "busy". Raise it on a slow or network disk. |
| `DB_ACQUIRE_TIMEOUT_MS` | `10000` | How long a request waits for a free pooled connection. |
| `DB_SYNCHRONOUS` | `normal` | Commit durability: `normal` (fsync at WAL checkpoints) or `full` (fsync every commit — safer against OS/power loss, slower, holds the write lock longer). |
| `DB_WRITE_MAX_RETRIES` | `5` | Retries after a refused write. `0` disables retrying. |
| `DB_WRITE_RETRY_BASE_MS` | `20` | Delay before the first retry; doubles each attempt (±25% jitter). |
| `DB_WRITE_RETRY_MAX_MS` | `500` | Upper bound on that delay. |
| `DB_DELETE_BATCH` | `500` | Rows deleted per statement by the retention sweep. |
| `DB_BUSY_RETRY_AFTER_SECS` | `2` | `Retry-After` sent to SDKs when the write budget is spent. |

Symptoms and what to change:

- **`ingest deferred an event: database is busy` in the logs** (SDKs are being
  told to retry) — the instance is writing more than the disk can absorb. Raise
  `DB_BUSY_TIMEOUT_MS` and/or `DB_WRITE_MAX_RETRIES`, keep `DB_SYNCHRONOUS=normal`,
  and lower per-project retention so sweeps have less to delete.
- **Slow UI during retention sweeps** — lower `DB_DELETE_BATCH` (smaller lock
  holds, more statements).
- **Network filesystem (NFS/SMB)** — don't. SQLite locking is unreliable there;
  use a local volume.

## Error reporting (Sentry)

soika can report **its own** backend and frontend errors to a Sentry (or
Sentry-compatible) instance — useful for dogfooding and operating soika itself.
It is **disabled by default** and enabled by setting a single DSN shared by both
halves:

```sh
SENTRY_DSN=https://<key>@sentry.example.com/<project>
SENTRY_ENVIRONMENT=production   # optional environment tag
```

- **Backend:** the SDK is initialized at startup and captures panics plus
  `tracing` error events. Transport uses rustls (the `ring` provider, like the
  rest of the stack), so no OpenSSL is pulled into the musl build.
- **Frontend:** the DSN is served **only** from the session-authenticated
  `GET /api/client-config` endpoint, and the browser SDK initializes **after
  sign-in**. This keeps the DSN off every unauthenticated route — the tradeoff
  is that errors on the login/setup pages (before authentication) are not
  captured.
- **Errors only:** performance tracing is disabled on both halves.

> Note: the release build uses `panic = "abort"`, so panic events are captured
> on a best-effort basis and may not flush before the process exits;
> `tracing::error!` events are reported reliably.

## License

[MIT](LICENSE.md)

### Deleting teams

Deleting a team never deletes its projects. Move projects to another team in the
deletion dialog, or pass `move_projects_to=<team_id>` to `DELETE /api/teams/{id}`.
The transfer and deletion are atomic. Without a target, a team that still owns
projects returns HTTP 409. Project IDs, DSNs, events and mute rules are preserved.

### SSO-only authentication (0.9.0)

With `OAUTH_ENABLED=true`, only effective instance Owners can use passwords.
Other accounts must use SSO, including when accepting an invite. Existing local
sessions stop working when SSO is enabled. On the first verified SSO login to a
matching local non-Owner account, its password, passkeys and previous sessions
are removed atomically. Disabling SSO later does not restore those credentials.
Owners listed in `ADMIN_USERS` can use SSO and retain their password; other
Owners use passwords only. `EMAIL_LOGIN_ENABLED=false` disables passwords for
Owners too. The password form is under “Administrator sign-in”.

Passkeys require SSO to be disabled. Before upgrading an installation with both
`PASSKEY_ENABLED=true` and `OAUTH_ENABLED=true`, disable one: this combination
now fails startup. Stored passkeys remain available when SSO is later disabled,
except credentials removed during conversion of a local account to SSO-only.

## Configuration as code (GitOps)

Version 0.9.0 can manage projects and teams from a YAML file. Each type has one
source of truth, selected independently: `PROJECT_MANAGEMENT_SOURCE=ui|yaml` and
`TEAM_MANAGEMENT_SOURCE=ui|yaml`. Both default to `ui`: upgrading keeps existing
management behavior and does not read a file. Restart the service to apply a
changed file; there is no hot reload. The settings page shows sources, policies,
file status and the last synchronization report.

| Projects | Teams | Management |
|---|---|---|
| ui | ui | Existing UI management; no file needed |
| yaml | ui | Projects reference teams created in the UI; invites work |
| ui | yaml | Create projects in the UI within file-defined teams |
| yaml | yaml | Projects, teams and memberships come from the file |

`NOTIFICATION_MANAGEMENT_SOURCE` independently selects `ui` or `yaml` for notification settings; see [Notifications](NOTIFICATIONS.md).

`CONFIG_PATH` defaults to `/etc/soika/config.yaml`. A readable, valid file is
required whenever either source is `yaml`. `version: 1` and the section for each
YAML-managed type are mandatory; empty sections must be explicit. Unknown fields,
duplicate keys/entities, invalid slugs, retention values and webhook URLs fail
startup. Sections for UI-managed types are ignored. For local development use
`CONFIG_PATH=deploy/config.yaml`.

```yaml
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

Members have role `admin` or `contributor`. A team must specify `members` or
`sso_groups`; use `members: []` for an intentionally empty team. File projects
require `slug`, `name` and `team`. Missing retention settings use
`DEFAULT_EVENTS_RETENTION` and zero days; a missing webhook disables that channel.
`${VAR}` in string values reads the process environment; unset variables fail
startup. Environment values are expanded as strings after YAML parsing, so a
secret cannot inject YAML fields. Do not put DSNs or credentials in the file.

Synchronization runs after migrations and before serving requests, in one
transaction with a database lock. An error rolls back changes. A project with
an existing slug keeps its ID, DSN, events, issues, favorites and mute rules;
configured fields are replaced. Repeated synchronization leaves unchanged
project timestamps intact. DSNs are generated only when creating projects.

### Removing entities and changing modes

`PROJECT_ORPHAN_POLICY=readonly|delete` and `TEAM_ORPHAN_POLICY=readonly|delete`
default to `readonly` and apply to **every** database entity absent from the
corresponding YAML section, including entities previously created in the UI.

For projects, `readonly` preserves data and retention but rejects both Sentry
store and envelope ingestion with HTTP 403 (`project is read-only`). Restoring
the same slug in the file restores ingestion with the same DSN. For teams,
`readonly` preserves membership and access. `delete` permanently deletes missing
projects and their data; missing teams can be deleted only after all their
projects have been moved or deleted. A team never cascades project deletion.
Switching either source back to `ui` clears that type's orphan state on startup.

Before enabling YAML management, list all entities you want to preserve, start
with `readonly`, inspect the settings page, and only then consider `delete`.
Changing a project's slug creates a different project with a **new DSN**; the
old project follows the orphan policy. Changing a team name similarly creates a
new team. There is no rename mapping or mass-deletion confirmation.

The Docker image contains a root-owned, read-only file with empty `projects`
and `teams` sections. Compose and Helm mount a file over it. If YAML management
is enabled without supplying a populated file, every existing entity follows
its orphan policy. **With `delete`, this loses data permanently.** The service
warns when using the exact default file, and the settings page marks it.

### Team grants and SSO

With YAML-managed teams, registration, invites and default-team management are
disabled. `/setup` still creates the initial Owner. File entries do not create
users: verified SSO login creates their accounts, applies memberships and
automatically approves users with matching grants even when
`OAUTH_REQUIRE_APPROVAL=true`. Email grants also apply to local accounts already
present before switching modes. Review local-account grant counts in settings;
team pages show pending email grants and membership sources.

SSO group grants require `OAUTH_ENABLED=true` and `OAUTH_PROVIDER=oidc`; they are
unavailable for GitHub OAuth. `OAUTH_GROUPS_CLAIM` defaults to `groups`. Soika
reads the claim from the verified ID token, otherwise from the provider's
userinfo endpoint, checking that its subject matches. Configure the claim name
for your GitLab version (for example `groups_direct` for an ID-token claim).
Paths match exactly, including letter case: parent groups and subgroups never imply
each other. Email and group grants combine using the highest role.

Each SSO login saves a group snapshot and recomputes membership; startup uses
these snapshots when rules change. Removal from a GitLab group takes effect on
the person's **next SSO login**, not immediately for an existing session. Group
snapshots are personal data and are removed with their account. The security migration clears older, case-folded snapshots because their original group names cannot be recovered safely; users with group-only access must sign in through SSO again after upgrading. Logs record
user IDs and group counts, never group paths or email addresses. Without OAuth,
a warning explains that new users cannot join YAML-managed teams.

Mute settings (project mute, tag rules and issue mute), issues, favorites and DSN
regeneration remain available in the UI with existing authorization. File-owned
management fields are read-only and their API mutations return HTTP 403.

### Upgrading to 0.9.0

Both management sources default to `ui`, so no configuration file is needed.
Team deletion now requires an explicit transfer target when projects remain.
Passkeys and SSO are mutually exclusive: disable one before upgrading if both
were enabled. Under SSO, old local non-Owner sessions are rejected and the first
SSO login converts matching local accounts to SSO-only, revoking their password,
passkeys and previous sessions. Effective Owners retain the documented fallback.

## SPA configuration API

`GET /api/config` returns the public sign-in policy and, for a signed-in
session, the profile, telemetry, accessible project summaries and team summaries.
Each team summary includes the caller's team `role`; instance Owner/Manager
privileges are carried by the profile. When enabled, `passkeys` lists only the
caller's credential metadata. Only instance Owners and Managers receive the
`admin` object (service settings and user directory); this key is omitted for
other callers. Responses use `Cache-Control: private, no-store` and `Vary: Cookie`.
The SPA reuses this configuration across navigation and reloads it after mutations
that invalidate page data. Existing resource GET routes and `/auth/config` remain
available for compatibility. Team details, filtered issues, event payloads and
invite previews continue to load on demand. Hovering a navigation link does not
prefetch page data.

Membership writes and account deletion enforce last-team-Admin protection in the
repository transaction, including re-adding an existing member and accepting an
invite. Before deleting an account that is a team's last Admin, assign another
Admin. Removing or demoting the last stored instance Owner is also refused within
the write transaction. SQLite takes an immediate write lock; PostgreSQL serializes
these infrequent administrative changes with table locks.


## Browser requests and security boundaries

Set `BASE_URL` to the exact public origin used by the SPA, including the port
when applicable. State-changing `/auth/*` and cookie API requests require a
matching `Origin` or `Referer`. If both are absent, automated clients must send
`X-Soika-Request: 1`. Null or foreign origins and cross-site Fetch Metadata are
rejected. The SPA sends the fallback header automatically; ingestion retains
its separate permissive CORS policy. Reverse proxies must preserve the source
origin headers. SSO callbacks use their state/nonce/PKCE checks instead.

Authentication uses the same session policy for every API resource: pending
accounts cannot use existing sessions or acquire a session by password,
passkey, or invite acceptance. Password failures for SSO accounts are throttled
and do not disclose whether an email exists. Local unverified registrations
cannot claim configured Owner identities. Addressed invitations require the
matching account, and accepting an invite consumes it and grants team access in
one transaction. Contributor invitations never demote an existing team Admin.

Login throttling and event IP enrichment use only the TCP peer, ignoring
`X-Forwarded-For` and `X-Real-IP`. Behind a reverse proxy, clients share its IP
bucket; password buckets additionally include the normalized email. Apply any
per-client throttling at the proxy. Login failure tracking is bounded; new
account names share an overflow budget when the table is full. These in-memory
limits reset on restart and are not shared across replicas.

Session-dependent API/auth responses use `Cache-Control: private, no-store`.
Responses disallow framing and MIME sniffing and use `Referrer-Policy:
no-referrer` to avoid leaking invitation URLs. Internal errors return a generic
message; operators diagnose the cause from server logs. OIDC and GitHub HTTP
calls have a 15-second timeout and do not follow redirects.

The [security audit](SECURITY_AUDIT.md) records the fixes, dependency patch and
remaining deployment assumptions.
