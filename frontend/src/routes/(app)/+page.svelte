<script lang="ts">
  import { goto, invalidateAll } from '$app/navigation';
  import { projects as projectsApi, errorMessage } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import type { Project } from '$lib/api';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import * as Dialog from '$lib/components/ui/dialog';
  import { toast } from '$lib/components/ui/sonner';
  import FolderPlus from '@lucide/svelte/icons/folder-plus';
  import BellOff from '@lucide/svelte/icons/bell-off';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Star from '@lucide/svelte/icons/star';
  import PageTitle from '$lib/components/page-title.svelte';
  import { teamFilterOptions, visibleProjects, type SortMode } from '$lib/dashboard';
  import { navActions } from '$lib/stores/nav-actions.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  const overviews = $derived(data.overviews);
  const teams = $derived(data.teams);

  // --- Favorites (local state for optimistic updates) ---
  let favoritedIds = $state(new Set<string>());
  $effect(() => {
    favoritedIds = new Set(overviews.filter((o) => o.favorited).map((o) => o.project.id));
  });

  async function toggleFavorite(e: MouseEvent, projectId: string) {
    e.preventDefault();
    e.stopPropagation();
    const wasFav = favoritedIds.has(projectId);
    const previous = new Set(favoritedIds);
    // Optimistic update
    const next = new Set(favoritedIds);
    if (wasFav) next.delete(projectId);
    else next.add(projectId);
    favoritedIds = next;
    try {
      await projectsApi.favorite(projectId, !wasFav);
    } catch (err) {
      favoritedIds = previous;
      toast.error(errorMessage(err, 'Failed to update favorite'));
      reportUnexpected(err);
    }
  }

  // --- Search with debounce ---
  let searchQuery = $state('');
  let debouncedQuery = $state('');

  $effect(() => {
    const q = searchQuery;
    const timer = setTimeout(() => {
      debouncedQuery = q;
    }, 300);
    return () => clearTimeout(timer);
  });

  // --- Sort mode ---
  let sortMode = $state<SortMode>('activity');

  // --- Team / "with issues" filters ---
  let filterTeamId = $state('');
  let onlyWithIssues = $state(false);

  const teamOptions = $derived(teamFilterOptions(overviews, teams));

  // A team can disappear from the options (project moved or deleted, reload with
  // a narrower project list) — drop a stale selection so the grid never silently
  // filters down to nothing.
  $effect(() => {
    if (filterTeamId && !teamOptions.some((t) => t.id === filterTeamId)) {
      filterTeamId = '';
    }
  });

  // Preview of what the toggle yields: counted against the other active filters
  // (search + team), so the badge never promises projects the grid would hide.
  const withIssuesCount = $derived(
    visibleProjects(overviews, {
      query: debouncedQuery,
      teamId: filterTeamId,
      onlyWithIssues: true,
      sortMode,
      favoritedIds
    }).length
  );

  const filteredAndSorted = $derived(
    visibleProjects(overviews, {
      query: debouncedQuery,
      teamId: filterTeamId,
      onlyWithIssues,
      sortMode,
      favoritedIds
    })
  );

  const filtersActive = $derived(
    debouncedQuery.trim() !== '' || filterTeamId !== '' || onlyWithIssues
  );

  function resetFilters() {
    searchQuery = '';
    debouncedQuery = '';
    filterTeamId = '';
    onlyWithIssues = false;
  }

  // --- Create-project dialog state ---
  let createOpen = $state(false);

  $effect(() => {
    navActions.newProjectCallback = () => (createOpen = true);
    return () => {
      navActions.newProjectCallback = null;
    };
  });
  let name = $state('');
  let teamId = $state('');
  let submitting = $state(false);

  // Default the team selection to the first available team when the dialog opens.
  $effect(() => {
    if (createOpen && !teamId && teams.length > 0) {
      teamId = teams[0].id;
    }
  });

  const canCreate = $derived(teams.length > 0);

  async function createProject(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim() || !teamId) return;
    submitting = true;
    try {
      const project: Project = await projectsApi.create({ name: name.trim(), team_id: teamId });
      toast.success(`Project "${project.name}" created`);
      createOpen = false;
      name = '';
      await invalidateAll();
      await goto(`/projects/${project.id}`);
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to create project'));
      reportUnexpected(err);
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title="Dashboard" />

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Projects</h1>
    <p class="text-muted-foreground text-sm">
      {#if overviews.length === 0}
        No projects yet — create your first to start capturing errors.
      {:else if filtersActive}
        Showing {filteredAndSorted.length} of {overviews.length}
        {overviews.length === 1 ? 'project' : 'projects'}.
      {:else}
        {overviews.length}
        {overviews.length === 1 ? 'project' : 'projects'} in your workspace.
      {/if}
    </p>
  </div>

  <Dialog.Root bind:open={createOpen}>
    <Dialog.Content class="sm:max-w-md">
      <Dialog.Header>
        <Dialog.Title>Create project</Dialog.Title>
        <Dialog.Description>
          A project owns a DSN and groups the errors your SDK sends.
        </Dialog.Description>
      </Dialog.Header>

      {#if !canCreate}
        <p class="text-muted-foreground text-sm">
          You need a team before creating a project.
          <a href="/teams" class="text-foreground underline-offset-4 hover:underline">
            Create a team
          </a>
          first.
        </p>
      {:else}
        <form onsubmit={createProject} class="space-y-4">
          <div class="space-y-2">
            <label for="project-name" class="text-sm font-medium">Name</label>
            <Input
              id="project-name"
              bind:value={name}
              placeholder="my-service"
              autocomplete="off"
              required
            />
          </div>
          <div class="space-y-2">
            <label for="project-team" class="text-sm font-medium">Team</label>
            <select
              id="project-team"
              bind:value={teamId}
              class="border-input focus-visible:border-primary focus-visible:ring-primary/30 flex h-9 w-full rounded-md border bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:ring-1 focus-visible:outline-none"
            >
              {#each teams as team (team.id)}
                <option value={team.id}>{team.name}</option>
              {/each}
            </select>
          </div>
          <Dialog.Footer>
            <Button type="submit" disabled={submitting || !name.trim()}>
              {submitting ? 'Creating…' : 'Create project'}
            </Button>
          </Dialog.Footer>
        </form>
      {/if}
    </Dialog.Content>
  </Dialog.Root>

  {#if overviews.length === 0}
    <Card.Root>
      <Card.Header>
        <Card.Title>No projects</Card.Title>
        <Card.Description>
          Create a project to get a DSN and start sending events from your application.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        <Button class="gap-2" onclick={() => (createOpen = true)} disabled={!canCreate}>
          <FolderPlus class="size-4" />
          New project
        </Button>
        {#if !canCreate}
          <p class="text-muted-foreground mt-3 text-sm">
            No teams available yet —
            <a href="/teams" class="text-foreground underline-offset-4 hover:underline">
              create a team
            </a>
            to continue.
          </p>
        {/if}
      </Card.Content>
    </Card.Root>
  {:else}
    <div class="flex flex-wrap items-center gap-3">
      <Input
        bind:value={searchQuery}
        placeholder="Search projects…"
        class="max-w-sm"
        aria-label="Search projects"
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            searchQuery = '';
            (e.target as HTMLInputElement).blur();
          }
        }}
      />
      {#if teamOptions.length > 1}
        <select
          bind:value={filterTeamId}
          class="border-input bg-background h-9 rounded-md border px-2 text-sm"
          aria-label="Filter by team"
        >
          <option value="">All teams</option>
          {#each teamOptions as team (team.id)}
            <option value={team.id}>{team.name}</option>
          {/each}
        </select>
      {/if}

      <button
        type="button"
        aria-pressed={onlyWithIssues}
        onclick={() => (onlyWithIssues = !onlyWithIssues)}
        class="border-input flex h-9 items-center gap-1.5 rounded-md border px-3 text-sm transition-colors {onlyWithIssues
          ? 'bg-secondary text-secondary-foreground font-medium'
          : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'}"
      >
        <CircleAlert class="size-4 {onlyWithIssues ? 'text-destructive' : ''}" />
        Projects with issues
        <span class="text-muted-foreground font-normal tabular-nums">{withIssuesCount}</span>
      </button>

      <div
        class="border-input flex h-9 rounded-md border text-sm"
        role="group"
        aria-label="Sort order"
      >
        <button
          class="flex items-center rounded-l-md px-3 transition-colors {sortMode === 'activity'
            ? 'bg-secondary text-secondary-foreground font-medium'
            : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'}"
          onclick={() => (sortMode = 'activity')}
        >
          By activity
        </button>
        <button
          class="border-input flex items-center rounded-r-md border-l px-3 transition-colors {sortMode ===
          'name'
            ? 'bg-secondary text-secondary-foreground font-medium'
            : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'}"
          onclick={() => (sortMode = 'name')}
        >
          By name
        </button>
      </div>
    </div>

    {#if filteredAndSorted.length === 0}
      <div class="flex flex-wrap items-center gap-3">
        <p class="text-muted-foreground text-sm">No projects match the current filters.</p>
        <Button variant="outline" size="sm" onclick={resetFilters}>Clear filters</Button>
      </div>
    {:else}
      <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {#each filteredAndSorted as { project, unresolvedCount } (project.id)}
          <a
            href={`/projects/${project.id}`}
            class="group focus-visible:ring-primary/30 rounded-lg transition focus-visible:ring-1 focus-visible:outline-none"
          >
            <Card.Root
              class="group-hover:border-primary/50 group-hover:ring-primary/20 h-full transition-colors group-hover:ring-2"
            >
              <Card.Header>
                <div class="flex items-start justify-between gap-2">
                  <div class="min-w-0">
                    <Card.Title class="truncate">{project.name}</Card.Title>
                    <Card.Description class="truncate font-mono text-xs">
                      {project.slug}
                    </Card.Description>
                  </div>
                  <div class="flex shrink-0 items-center gap-2">
                    {#if project.muted}
                      <BellOff
                        class="text-muted-foreground size-4"
                        aria-label="Notifications muted"
                      />
                    {/if}
                    <button
                      onclick={(e) => toggleFavorite(e, project.id)}
                      aria-label={favoritedIds.has(project.id)
                        ? 'Remove from favorites'
                        : 'Add to favorites'}
                      class="hover:bg-accent rounded p-1 transition-colors"
                    >
                      <Star
                        class="size-4 transition-colors {favoritedIds.has(project.id)
                          ? 'fill-amber-400 text-amber-400'
                          : 'text-muted-foreground'}"
                      />
                    </button>
                  </div>
                </div>
              </Card.Header>
              <Card.Content>
                {#if unresolvedCount === 0}
                  <p class="text-muted-foreground flex items-center gap-1.5 text-sm">
                    <CircleCheck class="size-4 text-emerald-500" />
                    No unresolved issues
                  </p>
                {:else}
                  <p class="flex items-center gap-1.5 text-sm font-medium">
                    <CircleAlert class="text-destructive size-4" />
                    {unresolvedCount}
                    {unresolvedCount === 1 ? 'unresolved issue' : 'unresolved issues'}
                  </p>
                {/if}
              </Card.Content>
            </Card.Root>
          </a>
        {/each}
      </div>
    {/if}
  {/if}
</div>
