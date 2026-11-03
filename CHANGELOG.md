# Changelog

## 0.10.0

- **Breaking:** project webhooks now send the Alertmanager webhook v4 JSON format
  instead of the previous flat payload. Update receivers before upgrading. See
  `docs/WEBHOOKS.md`.
- Added an Alertmanager notification channel that posts alerts to the Alertmanager
  API. It is gated by `NOTIFICATIONS_ALERTMANAGER_ENABLED`, configured with
  `ALERTMANAGER_URL`, supports default and per-project labels, optional TLS
  verification skipping, and test alerts from the admin UI.
- Service-wide notification settings can be managed through environment variables.
- **Breaking:** team invites are now controlled by `ALLOW_INVITES` and are off by
  default. Set `ALLOW_INVITES=true` to keep creating and accepting invite links.
- Invites stay off while teams are managed by the YAML file, whatever `ALLOW_INVITES`
  says. Team pages show a disabled invite button with an explanation.
- Admin settings pages explain options with info tooltips, and read-only default
  teams no longer show Save and Reset.

## 0.9.0

- Manage projects, teams and team membership declaratively from a YAML file,
  independently from UI management. Existing projects retain their DSNs and history.
- Assign team roles through email grants and exact OIDC group paths, including
  users joining later through SSO. Matching grants automatically approve users.
- Preserve removed projects as read-only by default, or explicitly choose permanent
  deletion. Restore read-only projects with the same DSN by adding them back.
- Display management sources, synchronization status, pending grants and orphan
  warnings in the UI. Mute settings and DSN regeneration remain available.
- Team deletion preserves projects and supports an explicit atomic transfer.
- Enforce SSO for non-Owner accounts, including old sessions and invite acceptance.
  The first SSO login to a local non-Owner account removes its password, passkeys
  and previous sessions. Owners retain their password fallback.
- **Breaking:** SSO and passkeys cannot be enabled together. Disable one before
  upgrading. Management defaults to the UI, so GitOps remains opt-in.
