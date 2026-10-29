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
Listing an address does not create an account or approve a pending one. Listed
local accounts may use their password even when SSO is enabled; listed accounts
may use SSO even when their stored role is `Owner`. Unlisted stored Owners retain
the password-only SSO protection.

`ADMIN_USERS` overrides permissions without changing the role stored in the
database. The current-user profile reports the effective role; **Admin → Users**
shows and edits the stored role. Only an Owner can edit or delete a listed account.
An Owner can change its stored role while the environment grant remains active.
After changing the list, restart the service: removing an address immediately
restores its stored role on subsequent requests, including existing sessions.
Roles assigned through the UI are preserved. Last-Owner protection continues to
count stored Owners, ensuring that the setup/recovery account remains available
independently of this configuration.

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

## Email and webhook notifications

Outgoing email is disabled by default. Set `SMTP_ENABLED=true` and configure
`SMTP_HOST` (plus any required SMTP credentials) to enable email notifications,
invite delivery and the admin test-email action. The Admin → SMTP card shows
whether delivery is enabled and configured.

Project webhooks are also disabled by default. Set `WEBHOOKS_ENABLED=true` to
send project issue and regression notifications to configured webhook URLs.
When disabled, the webhook URL field in project settings is visually disabled
and the service does not send webhook requests. These switches are read at
startup; restart the service after changing them.

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
  alongside the email/password form for Owners. Self-registration is disabled.
- A user is matched by the verified email from the provider; if no account
  exists, a new user is auto-provisioned with the `Member` instance role and no
  team membership — access is granted later by adding the user to a team (which
  grants access to that team's projects), either directly or via a team invite.
- Use `OAUTH_ALLOWED_EMAIL_DOMAINS` to restrict which email domains may
  auto-provision, since auto-creation trusts the provider's email claim.
- Stored instance Owners and local accounts listed in `ADMIN_USERS` keep
  password login while `EMAIL_LOGIN_ENABLED=true`, so the instance remains
  accessible if SSO is misconfigured.

### Disable email/password sign-in

Set `EMAIL_LOGIN_ENABLED=false` and restart soika to hide the email/password form
and reject `POST /auth/login` with `403` for every account, including Owners and
accounts listed in `ADMIN_USERS`. The public `/auth/config` response exposes this
setting as `password_login_enabled`. SSO and passkeys remain available.

This also disables public password registration and anonymous password-based
invite acceptance, preventing alternative password sign-in paths. Invitees must
sign in via SSO or a passkey first, then accept the invite. Existing sessions and
first-run administrator setup are unaffected. Configure SSO or passkeys before
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

Passkeys coexist with everything else: password login and SSO keep working, and
an account with no registered key is unaffected. Deleting a user deletes their
credentials with them.

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
