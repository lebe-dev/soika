<script lang="ts" module>
  import { translate } from '$lib/i18n';
  // One-line descriptions of what each role can do, kept in sync with the
  // permission matrix in README.md. Surfaced next to role selectors so the
  // role names (Owner / Manager / Member, Admin / Contributor) are not opaque.
  type RoleEntry = { name: string; description: string };

  const INSTANCE_ROLES: RoleEntry[] = [
    {
      name: 'owner',
      description:
        'full_control_everything_a_manager_can_do_plus_granting_revoking_owner_and_destru'
    },
    {
      name: 'manager',
      description:
        'administers_every_team_and_project_invites_and_approves_users_sets_instance_role'
    },
    {
      name: 'member',
      description:
        'no_instance_wide_powers_access_is_limited_to_the_projects_of_the_teams_they_belo'
    }
  ];

  const TEAM_ROLES: RoleEntry[] = [
    {
      name: 'admin',
      description:
        'manages_the_team_members_and_their_roles_project_settings_and_invites_for_this_t'
    },
    {
      name: 'contributor',
      description: 'read_and_triage_the_team_s_projects_view_issues_and_events_no_team_management'
    }
  ];

  // One-line description for a single team role (`admin | contributor`), used
  // where only one role is shown (e.g. the invite acceptance preview).
  export function teamRoleDescription(role: 'admin' | 'contributor'): string {
    const name = role === 'admin' ? 'admin' : 'contributor';
    const key = TEAM_ROLES.find((r) => r.name === name)?.description;
    return key ? translate(key) : '';
  }
</script>

<script lang="ts">
  import { t } from '$lib/i18n';
  let { scope }: { scope: 'instance' | 'team' } = $props();

  const roles = $derived(scope === 'instance' ? INSTANCE_ROLES : TEAM_ROLES);
</script>

<dl class="text-muted-foreground space-y-1 text-xs">
  {#each roles as role (role.name)}
    <div class="flex gap-1.5">
      <dt class="text-foreground shrink-0 font-medium">{$t(role.name)}</dt>
      <dd>— {$t(role.description)}</dd>
    </div>
  {/each}
</dl>
