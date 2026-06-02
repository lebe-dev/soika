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
  exists, a new **regular** user is auto-provisioned (`is_admin=false`, no project
  or team membership — access is granted later by an admin via invite/members).
- Use `OAUTH_ALLOWED_EMAIL_DOMAINS` to restrict which email domains may
  auto-provision, since auto-creation trusts the provider's email claim.
- **Only the built-in admin** (created on first run via the `/setup` page)
  keeps password login, so the instance remains accessible if SSO is
  misconfigured.

## License

[MIT](LICENSE.md)
