# Frequently asked questions

## What happens when I switch `SERVICE_MANAGEMENT_SOURCE` between `ui` and `env`?

The switch selects who controls general service settings. It takes effect after
restarting Soika or rolling out a new deployment. The default is `ui`; the accepted
values are `ui` and `env`.

### Switching from `ui` to `env`

At startup, Soika **overwrites the saved settings in the database** with values
from the environment:

| Setting | Environment variable | Default when omitted |
| --- | --- | --- |
| Organization name | `ORGANIZATION_NAME` | `soika` |
| Public registration | `ALLOW_SIGNUP` | `false` |
| Default teams for new users | `DEFAULT_TEAM_IDS` | Empty list |
| Alertmanager channel enabled | `NOTIFICATIONS_ALERTMANAGER_ENABLED` | `false` |
| Alertmanager URL | `ALERTMANAGER_URL` | Empty |

`DEFAULT_TEAM_IDS` accepts comma-separated UUIDs of existing teams. It affects
new accounts; switching modes does not change existing users' team memberships.
Enabling Alertmanager requires a valid URL. Invalid configuration prevents
startup.

Alertmanager labels and annotation templates are reset to the built-in defaults.
Previously saved UI settings and custom templates are not kept as a separate
backup. Record any values you may want to restore before switching.

These settings become read-only in the UI and API, including for Owners and
Managers. The UI displays “Configured through environment variables”. The
Alertmanager test-send action is also blocked. Every subsequent startup in
`env` mode reapplies the environment values.

### Switching from `env` to `ui`

Soika keeps the values last written to the database by `env` mode and allows
editing them through the UI again. **It does not restore the settings that
existed before you switched to `env`.** This includes notification templates:
the built-in defaults remain until you customize them again.

For example, if the organization was `Acme` in UI mode and `ORGANIZATION_NAME=Example`
was applied in env mode, switching back to UI leaves the organization named
`Example`. You can then rename it in the admin UI.

Configuration-only environment variables still apply in UI mode.
`NOTIFICATIONS_ALERTMANAGER_ENABLED=false` still disables the Alertmanager feature,
regardless of the channel's saved enabled state. `ALERTMANAGER_URL` remains the
fallback when the saved URL is empty. A URL written to the database by env mode
remains an explicit saved URL until you edit or clear it.

### What remains under its existing rules?

Mute controls, issue triage, user administration, and project/team management keep
their existing permissions. `PROJECT_MANAGEMENT_SOURCE` and
`TEAM_MANAGEMENT_SOURCE` remain independent. Switching the service setting does
not delete projects, issues, events, users, or teams. Existing Alertmanager alerts
follow the normal [notification lifecycle](NOTIFICATIONS.md).

Set `SERVICE_MANAGEMENT_SOURCE=ui` explicitly when switching back. If you remove
the variable instead, the legacy `NOTIFICATION_MANAGEMENT_SOURCE` variable can
still select YAML management for notifications.

See [installation and configuration](OPERATIONS.md) and
[notifications](NOTIFICATIONS.md) for the full configuration details.
