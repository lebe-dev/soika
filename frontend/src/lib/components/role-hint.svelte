<script lang="ts" module>
  // One-line descriptions of what each role can do, kept in sync with the
  // permission matrix in README.md. Surfaced next to role selectors so the
  // role names (Owner / Manager / Member, Admin / Contributor) are not opaque.
  type RoleEntry = { name: string; description: string };

  const INSTANCE_ROLES: RoleEntry[] = [
    {
      name: 'Owner',
      description:
        'Full control. Everything a Manager can do, plus granting/revoking Owner and destructive instance operations.'
    },
    {
      name: 'Manager',
      description:
        'Administers every team and project, invites and approves users, sets instance roles (but cannot grant Owner).'
    },
    {
      name: 'Member',
      description:
        'No instance-wide powers. Access is limited to the projects of the teams they belong to.'
    }
  ];

  const TEAM_ROLES: RoleEntry[] = [
    {
      name: 'Admin',
      description:
        'Manages the team: members and their roles, project settings, and invites for this team.'
    },
    {
      name: 'Contributor',
      description:
        'Read and triage the team’s projects (view issues and events). No team management.'
    }
  ];

  // One-line description for a single team role (`admin | contributor`), used
  // where only one role is shown (e.g. the invite acceptance preview).
  export function teamRoleDescription(role: 'admin' | 'contributor'): string {
    const name = role === 'admin' ? 'Admin' : 'Contributor';
    return TEAM_ROLES.find((r) => r.name === name)?.description ?? '';
  }
</script>

<script lang="ts">
  let { scope }: { scope: 'instance' | 'team' } = $props();

  const roles = $derived(scope === 'instance' ? INSTANCE_ROLES : TEAM_ROLES);
</script>

<dl class="text-muted-foreground space-y-1 text-xs">
  {#each roles as role (role.name)}
    <div class="flex gap-1.5">
      <dt class="text-foreground shrink-0 font-medium">{role.name}</dt>
      <dd>— {role.description}</dd>
    </div>
  {/each}
</dl>
