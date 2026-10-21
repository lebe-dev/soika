<script lang="ts">
  // Roster of teams. Each row is a plain link so SvelteKit's router handles
  // the click as a client-side navigation (no full page reload).
  import type { TeamSummary } from '$lib/api';
  import * as Card from '$lib/components/ui/card';
  import Avatar from '$lib/components/avatar.svelte';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';

  let { teams }: { teams: TeamSummary[] } = $props();
</script>

<Card.Root class="gap-0 py-0 shadow-sm">
  <ul class="divide-border/60 divide-y">
    {#each teams as team (team.id)}
      <li>
        <a
          href={`/teams/${team.id}`}
          class="group hover:bg-muted/50 focus-visible:bg-muted/50 flex items-center gap-3 px-4 py-3 transition-colors outline-none"
        >
          <Avatar name={team.name} id={team.id} shape="tile" />
          <div class="min-w-0 flex-1">
            <p class="truncate font-medium">{team.name}</p>
            <p class="text-muted-foreground text-sm">
              {team.member_count}
              {team.member_count === 1 ? 'member' : 'members'} · {team.project_count}
              {team.project_count === 1 ? 'project' : 'projects'}
            </p>
          </div>
          <ChevronRight
            class="text-muted-foreground/60 group-hover:text-foreground size-4 shrink-0 transition-colors"
          />
        </a>
      </li>
    {/each}
  </ul>
</Card.Root>
