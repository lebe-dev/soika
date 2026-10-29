<script lang="ts">
  import { t } from '$lib/i18n';
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
  import ArrowRight from '@lucide/svelte/icons/arrow-right';
  import Star from '@lucide/svelte/icons/star';
  import PageTitle from '$lib/components/page-title.svelte';
  import PageHeader from '$lib/components/page-header.svelte';
  import Avatar from '$lib/components/avatar.svelte';
  import SegmentedControl from '$lib/components/segmented-control.svelte';
  import { NativeSelect } from '$lib/components/ui/native-select';
  import { teamFilterOptions, visibleProjects, type SortMode } from '$lib/dashboard';
  import { readJson, writeJson } from '$lib/local-storage';
  import type { PageData } from './$types';

  const FILTERS_STORAGE_KEY = 'soika:dashboard:filters';

  interface StoredDashboardFilters {
    query: string;
    teamId: string;
    onlyWithIssues: boolean;
    sortMode: SortMode;
  }

  const storedFilters = readJson<Partial<StoredDashboardFilters>>(FILTERS_STORAGE_KEY);

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
      toast.error(errorMessage(err, $t('failed_to_update_favorite')));
      reportUnexpected(err);
    }
  }

  // --- Search with debounce ---
  // Seeded from localStorage so returning to the dashboard restores the last
  // filters instead of always starting from a blank grid.
  const initialQuery = typeof storedFilters?.query === 'string' ? storedFilters.query : '';
  let searchQuery = $state(initialQuery);
  let debouncedQuery = $state(initialQuery);

  $effect(() => {
    const q = searchQuery;
    const timer = setTimeout(() => {
      debouncedQuery = q;
    }, 300);
    return () => clearTimeout(timer);
  });

  // --- Sort mode ---
  let sortMode = $state<SortMode>(storedFilters?.sortMode === 'name' ? 'name' : 'activity');

  // --- Team / "with issues" filters ---
  let filterTeamId = $state(typeof storedFilters?.teamId === 'string' ? storedFilters.teamId : '');
  let onlyWithIssues = $state(storedFilters?.onlyWithIssues === true);

  // Persist on every change (debounced query, so typing doesn't spam writes).
  $effect(() => {
    writeJson(FILTERS_STORAGE_KEY, {
      query: debouncedQuery,
      teamId: filterTeamId,
      onlyWithIssues,
      sortMode
    });
  });

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

  let name = $state('');
  let teamId = $state('');
  let submitting = $state(false);

  // Default the team selection to the first available team when the dialog opens.
  $effect(() => {
    if (createOpen && !teamId && teams.length > 0) {
      teamId = teams[0].id;
    }
  });

  const projectsManaged = $derived(data.telemetry?.project_management_source === 'yaml');
  const canCreate = $derived(teams.length > 0 && !projectsManaged);

  async function createProject(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim() || !teamId) return;
    submitting = true;
    try {
      const project: Project = await projectsApi.create({ name: name.trim(), team_id: teamId });
      toast.success($t('project_created', { values: { name: project.name } }));
      createOpen = false;
      name = '';
      await invalidateAll();
      await goto(`/projects/${project.id}`);
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_create_project')));
      reportUnexpected(err);
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title={$t('dashboard')} />

<div class="space-y-6">
  <PageHeader title={$t('projects')}>
    {#snippet description()}
      {#if overviews.length === 0}
        {$t('no_projects_yet_create_your_first_to_start_capturing_errors')}
      {:else if filtersActive}
        {$t('projects_filtered', {
          values: { shown: filteredAndSorted.length, count: overviews.length }
        })}
      {:else}
        {$t('projects_workspace', { values: { count: overviews.length } })}
      {/if}
    {/snippet}
    {#snippet actions()}
      {#if canCreate}
        <Button onclick={() => (createOpen = true)} class="gap-1.5">
          <FolderPlus class="size-4" />
          {$t('new_project')}
        </Button>
      {/if}
    {/snippet}
  </PageHeader>

  <Dialog.Root bind:open={createOpen}>
    <Dialog.Content class="sm:max-w-md">
      <Dialog.Header>
        <Dialog.Title>{$t('create_project')}</Dialog.Title>
        <Dialog.Description>
          {$t('a_project_owns_a_dsn_and_groups_the_errors_your_sdk_sends')}
        </Dialog.Description>
      </Dialog.Header>

      {#if !canCreate && !projectsManaged}
        <p class="text-muted-foreground text-sm">
          {$t('you_need_a_team_before_creating_a_project')}
          <a href="/teams" class="text-foreground underline-offset-4 hover:underline">
            {$t('create_a_team')}
          </a>
          {$t('first')}
        </p>
      {:else}
        <form onsubmit={createProject} class="space-y-4">
          <div class="space-y-2">
            <label for="project-name" class="text-sm font-medium">{$t('name')}</label>
            <Input
              id="project-name"
              bind:value={name}
              placeholder="my-service"
              autocomplete="off"
              required
            />
          </div>
          <div class="space-y-2">
            <label for="project-team" class="text-sm font-medium">{$t('team')}</label>
            <NativeSelect id="project-team" bind:value={teamId} class="w-full">
              {#each teams as team (team.id)}
                <option value={team.id}>{team.name}</option>
              {/each}
            </NativeSelect>
          </div>
          <Dialog.Footer>
            <Button type="submit" disabled={submitting || !name.trim()}>
              {submitting ? $t('creating') : $t('create_project')}
            </Button>
          </Dialog.Footer>
        </form>
      {/if}
    </Dialog.Content>
  </Dialog.Root>

  {#if overviews.length === 0}
    <Card.Root>
      <Card.Header>
        <Card.Title>{$t('no_projects')}</Card.Title>
        <Card.Description>
          {projectsManaged
            ? $t('managed_by_configuration')
            : $t('create_a_project_to_get_a_dsn_and_start_sending_events_from_your_application')}
        </Card.Description>
      </Card.Header>
      <Card.Content>
        {#if !projectsManaged}
          <Button class="gap-2" onclick={() => (createOpen = true)} disabled={!canCreate}>
            <FolderPlus class="size-4" />
            {$t('new_project')}
          </Button>
        {/if}
        {#if !canCreate && !projectsManaged}
          <p class="text-muted-foreground mt-3 text-sm">
            {$t('no_teams_available_yet')}
            <a href="/teams" class="text-foreground underline-offset-4 hover:underline">
              {$t('create_a_team_316b5a')}
            </a>
            {$t('to_continue')}
          </p>
        {/if}
      </Card.Content>
    </Card.Root>
  {:else}
    <div class="flex flex-wrap items-center gap-3">
      <Input
        bind:value={searchQuery}
        placeholder={$t('search_projects')}
        class="max-w-sm {searchQuery.trim() !== ''
          ? 'border-primary/50 ring-primary/15 ring-1'
          : ''}"
        aria-label={$t('search_projects_30c3c3')}
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            searchQuery = '';
            (e.target as HTMLInputElement).blur();
          }
        }}
      />
      {#if teamOptions.length > 1}
        <NativeSelect
          bind:value={filterTeamId}
          class={filterTeamId ? 'border-primary/50 ring-primary/15 ring-1' : ''}
          aria-label={$t('filter_by_team')}
        >
          <option value="">{$t('all_teams')}</option>
          {#each teamOptions as team (team.id)}
            <option value={team.id}>{team.name}</option>
          {/each}
        </NativeSelect>
      {/if}

      <button
        type="button"
        aria-pressed={onlyWithIssues}
        onclick={() => (onlyWithIssues = !onlyWithIssues)}
        class="border-input bg-card flex h-8 items-center gap-2 rounded-lg border pr-1.5 pl-3 text-sm shadow-[0_1px_2px_rgb(20_23_28/0.03)] transition-colors {onlyWithIssues
          ? 'border-primary/50 ring-primary/15 font-medium ring-3'
          : 'hover:bg-accent'}"
      >
        {$t('projects_with_issues')}
        <span
          class="bg-destructive/10 text-destructive min-w-5 rounded-md px-1.5 text-center font-mono text-xs leading-5 tabular-nums"
        >
          {withIssuesCount}
        </span>
      </button>

      <SegmentedControl
        class="sm:ml-auto"
        label={$t('sort_order')}
        value={sortMode}
        onselect={(v) => (sortMode = v)}
        options={[
          { value: 'activity', label: $t('by_activity') },
          { value: 'name', label: $t('by_name') }
        ]}
      />
    </div>

    {#if filteredAndSorted.length === 0}
      <div class="flex flex-wrap items-center gap-3">
        <p class="text-muted-foreground text-sm">{$t('no_projects_match_the_current_filters')}</p>
        <Button variant="outline" size="sm" onclick={resetFilters}>{$t('clear_filters')}</Button>
      </div>
    {:else}
      <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {#each filteredAndSorted as { project, unresolvedCount } (project.id)}
          <a
            href={`/projects/${project.id}`}
            class="group bg-card focus-visible:ring-primary/30 hover:border-primary/35 flex flex-col rounded-xl border transition-[border-color,box-shadow] hover:shadow-[0_6px_20px_-12px_rgb(20_23_28/0.25)] focus-visible:ring-2 focus-visible:outline-none"
          >
            <div class="flex items-start gap-3 py-4 pr-3 pl-4">
              <Avatar name={project.name} id={project.id} shape="tile" />
              <div class="min-w-0 flex-1 pt-px">
                <div class="truncate text-[15px] font-medium tracking-[-0.01em]">
                  {project.name}
                </div>
                <div class="text-muted-foreground truncate font-mono text-xs">
                  {project.slug}
                </div>
              </div>
              <div class="flex shrink-0 items-center gap-1">
                {#if project.muted}
                  <BellOff
                    class="text-muted-foreground size-4"
                    aria-label={$t('notifications_muted')}
                  />
                {/if}
                <button
                  onclick={(e) => toggleFavorite(e, project.id)}
                  aria-label={favoritedIds.has(project.id)
                    ? $t('remove_from_favorites')
                    : $t('add_to_favorites')}
                  class="hover:bg-accent grid size-8 place-items-center rounded-lg transition-colors"
                >
                  <Star
                    class="size-4 transition-colors {favoritedIds.has(project.id)
                      ? 'fill-amber-400 text-amber-400'
                      : 'text-muted-foreground/60'}"
                  />
                </button>
              </div>
            </div>
            <div
              class="border-hairline mt-auto flex min-h-12 items-center justify-between gap-3 border-t px-4"
            >
              {#if unresolvedCount === 0}
                <span class="text-muted-foreground flex items-center gap-2 text-[13px]">
                  <span class="bg-success size-1.5 rounded-full"></span>
                  {$t('no_unresolved_issues')}
                </span>
              {:else}
                <span class="flex items-baseline gap-2">
                  <span
                    class="text-destructive text-xl font-medium tracking-[-0.02em] tabular-nums"
                  >
                    {unresolvedCount}
                  </span>
                  <span class="text-muted-foreground text-[13px]">
                    {$t('unresolved_issues_label', { values: { count: unresolvedCount } })}
                  </span>
                </span>
              {/if}
              <ArrowRight
                class="text-muted-foreground/50 group-hover:text-foreground size-4 transition-colors"
              />
            </div>
          </a>
        {/each}
      </div>
    {/if}
  {/if}
</div>
