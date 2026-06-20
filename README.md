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
