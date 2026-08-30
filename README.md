# soika

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

## SSO via OIDC

soika can delegate sign-in to an external OpenID Connect (OIDC) provider such as
GitLab, Keycloak, Authentik, Google, or Okta. SSO is **disabled by default**
(`OAUTH_ENABLED=false`); while disabled, the built-in password login behaves
exactly as before.

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

- The login screen shows a **"Sign in with {OAUTH_PROVIDER_NAME}"** button and
  hides the email/password form and self-registration.
- A user is matched by the verified email from the provider; if no account
  exists, a new user is auto-provisioned with the `Member` instance role and no
  team membership — access is granted later by adding the user to a team (which
  grants access to that team's projects), either directly or via a team invite.
- Use `OAUTH_ALLOWED_EMAIL_DOMAINS` to restrict which email domains may
  auto-provision, since auto-creation trusts the provider's email claim.
- **Only the instance `Owner`** (created on first run via the `/setup` page)
  keeps password login, so the instance remains accessible if SSO is
  misconfigured.

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
| `DATABASE_URL` | `sqlite://soika.db` | Database file; created if missing. |
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
