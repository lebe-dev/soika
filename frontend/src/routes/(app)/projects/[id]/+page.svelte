<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { projects, errorMessage, type Issue, type IssueStatus } from '$lib/api';
  import * as Card from '$lib/components/ui/card';
  import IssueStatusBadge from '$lib/components/issue-status-badge.svelte';
  import { cn } from '$lib/utils';
  import { formatCount, formatRelative } from '$lib/format';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { LayoutData } from './$types';
  import Inbox from '@lucide/svelte/icons/inbox';

  // Issues list for a project with status filter. The project is
  // provided by the parent layout load; issues are fetched reactively here so
  // the status filter does not require a full navigation/load.
  let { data }: { data: LayoutData } = $props();

  const projectId = $derived(data.project.id);

  type Filter = IssueStatus | 'all';
  const filters: { value: Filter; label: string }[] = [
    { value: 'unresolved', label: 'Unresolved' },
    { value: 'resolved', label: 'Resolved' },
    { value: 'muted', label: 'Muted' },
    { value: 'all', label: 'All' }
  ];

  // Seed the active filter from the URL (?status=) so links/back are stable.
  const urlStatus = $derived(
    ($page.url.searchParams.get('status') as Filter | null) ?? 'unresolved'
  );

  type Sort = 'last_seen' | 'event_count';
  const sorts: { value: Sort; label: string }[] = [
    { value: 'last_seen', label: 'Recent' },
    { value: 'event_count', label: 'Most frequent' }
  ];

  // Seed the active sort from the URL (?sort=) the same way as the status filter.
  const urlSort = $derived(($page.url.searchParams.get('sort') as Sort | null) ?? 'last_seen');

  // First-class field filters (Stories 4.4 / 5.2). `level` is a small set of
  // known severities; environment/release are free-form so they use text inputs.
  type Level = 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  const levels: { value: Level; label: string }[] = [
    { value: 'all', label: 'All levels' },
    { value: 'fatal', label: 'Fatal' },
    { value: 'error', label: 'Error' },
    { value: 'warning', label: 'Warning' },
    { value: 'info', label: 'Info' },
    { value: 'debug', label: 'Debug' }
  ];

  const urlLevel = $derived(($page.url.searchParams.get('level') as Level | null) ?? 'all');
  const urlEnvironment = $derived($page.url.searchParams.get('environment') ?? '');
  const urlRelease = $derived($page.url.searchParams.get('release') ?? '');

  let issues = $state<Issue[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  // Re-fetch whenever the project, status filter, sort, or field filters change.
  // For the default view the layout seed is shown immediately (no loading flash)
  // and a background fetch always runs to guarantee fresh data after mutations.
  $effect(() => {
    const status = urlStatus;
    const sort = urlSort;
    const level = urlLevel;
    const environment = urlEnvironment.trim();
    const release = urlRelease.trim();
    const id = projectId;

    const isDefaultView =
      status === 'unresolved' &&
      sort === 'last_seen' &&
      level === 'all' &&
      environment === '' &&
      release === '';

    // Seed from layout data for immediate render; show loading only for
    // non-default views that have no pre-loaded data.
    if (isDefaultView) {
      issues = data.issues;
      loading = false;
    } else {
      loading = true;
    }
    loadError = null;

    // Omit defaults/empties so URLs/requests stay clean (all status, last_seen
    // sort, all levels, no environment/release filter). buildUrl skips
    // undefined/null but NOT empty strings, so empty values must be omitted here.
    const query = {
      ...(status === 'all' ? {} : { status }),
      ...(sort === 'last_seen' ? {} : { sort }),
      ...(level === 'all' ? {} : { level }),
      ...(environment === '' ? {} : { environment }),
      ...(release === '' ? {} : { release })
    };
    let cancelled = false;
    projects
      .issues(id, query)
      .then((res) => {
        if (!cancelled) {
          issues = res;
          loading = false;
        }
      })
      .catch((err) => {
        if (!cancelled) {
          loadError = errorMessage(err, 'Failed to load issues');
          issues = [];
          loading = false;
        }
      });
    return () => {
      cancelled = true;
    };
  });

  // Set or delete a single URL search param, preserving the others. An empty /
  // null value (or a value equal to `clearOn`) removes the param so we never
  // send a blank filter to the API.
  function setParam(key: string, value: string | null, clearOn?: string) {
    const url = new URL($page.url);
    const trimmed = value?.trim() ?? '';
    if (trimmed === '' || trimmed === clearOn) url.searchParams.delete(key);
    else url.searchParams.set(key, trimmed);
    goto(url, { replaceState: true, keepFocus: true, noScroll: true });
  }

  function selectFilter(value: Filter) {
    setParam('status', value, 'unresolved');
  }

  function selectSort(value: Sort) {
    setParam('sort', value, 'last_seen');
  }

  function selectLevel(value: Level) {
    setParam('level', value, 'all');
  }
</script>

<PageTitle title={data.project.name} />

<div class="space-y-4">
  <div class="flex flex-wrap items-center gap-3">
    <div class="flex flex-wrap items-center gap-1">
      {#each filters as f (f.value)}
        <button
          type="button"
          onclick={() => selectFilter(f.value)}
          class={cn(
            'rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
            urlStatus === f.value
              ? 'bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:text-foreground'
          )}
        >
          {f.label}
        </button>
      {/each}
    </div>

    <div class="flex flex-wrap items-center gap-1 sm:ml-auto">
      {#each sorts as s (s.value)}
        <button
          type="button"
          onclick={() => selectSort(s.value)}
          class={cn(
            'rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
            urlSort === s.value
              ? 'bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:text-foreground'
          )}
        >
          {s.label}
        </button>
      {/each}
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-2">
    <select
      value={urlLevel}
      onchange={(e) => selectLevel(e.currentTarget.value as Level)}
      class="border-input bg-background h-9 rounded-md border px-2 text-sm"
      aria-label="Filter by level"
    >
      {#each levels as l (l.value)}
        <option value={l.value}>{l.label}</option>
      {/each}
    </select>

    <input
      type="text"
      value={urlEnvironment}
      onchange={(e) => setParam('environment', e.currentTarget.value)}
      placeholder="Environment"
      aria-label="Filter by environment"
      class="border-input bg-background h-9 w-40 rounded-md border px-2 text-sm"
    />

    <input
      type="text"
      value={urlRelease}
      onchange={(e) => setParam('release', e.currentTarget.value)}
      placeholder="Release"
      aria-label="Filter by release"
      class="border-input bg-background h-9 w-40 rounded-md border px-2 text-sm"
    />
  </div>

  {#if loading}
    <Card.Root>
      <Card.Content class="text-muted-foreground py-12 text-center text-sm"
        >Loading issues…</Card.Content
      >
    </Card.Root>
  {:else if loadError}
    <Card.Root>
      <Card.Content class="text-destructive py-12 text-center text-sm">{loadError}</Card.Content>
    </Card.Root>
  {:else if issues.length === 0}
    <Card.Root>
      <Card.Content class="flex flex-col items-center gap-3 py-16 text-center">
        <Inbox class="text-muted-foreground size-10" />
        <div>
          <p class="font-medium">No issues here</p>
          <p class="text-muted-foreground text-sm">
            {urlStatus === 'unresolved'
              ? 'No unresolved issues — nice and quiet.'
              : 'No issues match this filter.'}
          </p>
        </div>
      </Card.Content>
    </Card.Root>
  {:else}
    <div class="overflow-hidden rounded-lg border">
      {#each issues as issue (issue.id)}
        <a
          href={`/projects/${projectId}/issues/${issue.id}`}
          class="hover:bg-accent/40 flex items-center gap-4 border-b px-4 py-3 transition-colors last:border-b-0"
        >
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              {#if issue.level}
                <span
                  class="text-muted-foreground text-[10px] font-semibold tracking-wide uppercase"
                >
                  {issue.level}
                </span>
              {/if}
              <span class="truncate font-medium">{issue.title}</span>
            </div>
            {#if issue.culprit}
              <p class="text-muted-foreground truncate font-mono text-xs">{issue.culprit}</p>
            {/if}
          </div>

          <div class="hidden shrink-0 sm:block">
            <IssueStatusBadge status={issue.status} />
          </div>

          <div class="w-16 shrink-0 text-right">
            <div class="text-sm font-semibold tabular-nums">{formatCount(issue.event_count)}</div>
            <div class="text-muted-foreground text-[10px] tracking-wide uppercase">events</div>
          </div>

          <div class="text-muted-foreground hidden w-24 shrink-0 text-right text-xs md:block">
            {formatRelative(issue.last_seen)}
          </div>
        </a>
      {/each}
    </div>
    <p class="text-muted-foreground text-xs">
      {issues.length}
      {issues.length === 1 ? 'issue' : 'issues'}
    </p>
  {/if}
</div>
