<script lang="ts">
  import { t, locale } from '$lib/i18n';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { projects, errorMessage, type Issue, type IssueStatus } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import IssueStatusBadge from '$lib/components/issue-status-badge.svelte';
  import { cn } from '$lib/utils';
  import { formatCount, formatRelative } from '$lib/format';
  import PageTitle from '$lib/components/page-title.svelte';
  import { Input } from '$lib/components/ui/input';
  import { NativeSelect } from '$lib/components/ui/native-select';
  import type { LayoutData } from './$types';
  import Inbox from '@lucide/svelte/icons/inbox';
  import { readJson, writeJson } from '$lib/local-storage';
  import SegmentedControl from '$lib/components/segmented-control.svelte';

  // Issues list for a project with status filter. The project is
  // provided by the parent layout load; issues are fetched reactively here so
  // the status filter does not require a full navigation/load.
  let { data }: { data: LayoutData } = $props();

  const projectId = $derived(data.project.id);

  type Filter = IssueStatus | 'all';
  const filters: { value: Filter; label: string }[] = $derived([
    { value: 'unresolved', label: $t('status_unresolved') },
    { value: 'resolved', label: $t('status_resolved') },
    { value: 'muted', label: $t('status_muted') },
    { value: 'all', label: $t('all') }
  ]);

  // Seed the active filter from the URL (?status=) so links/back are stable.
  const urlStatus = $derived(
    ($page.url.searchParams.get('status') as Filter | null) ?? 'unresolved'
  );

  type Sort = 'last_seen' | 'event_count';
  const sorts: { value: Sort; label: string }[] = $derived([
    { value: 'last_seen', label: $t('recent') },
    { value: 'event_count', label: $t('most_frequent') }
  ]);

  // Seed the active sort from the URL (?sort=) the same way as the status filter.
  const urlSort = $derived(($page.url.searchParams.get('sort') as Sort | null) ?? 'last_seen');

  // First-class field filters (Stories 4.4 / 5.2). `level` is a small set of
  // known severities; environment/release are free-form so they use text inputs.
  type Level = 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  const levels: { value: Level; label: string }[] = $derived([
    { value: 'all', label: $t('all_levels') },
    { value: 'fatal', label: $t('fatal') },
    { value: 'error', label: $t('error') },
    { value: 'warning', label: $t('warning') },
    { value: 'info', label: $t('info') },
    { value: 'debug', label: $t('debug') }
  ]);

  const urlLevel = $derived(($page.url.searchParams.get('level') as Level | null) ?? 'all');
  const urlEnvironment = $derived($page.url.searchParams.get('environment') ?? '');
  const urlRelease = $derived($page.url.searchParams.get('release') ?? '');

  // Filters persist per project (environment/release values don't generalize
  // across projects) so leaving and returning to the Issues tab restores the
  // last view. The URL stays the source of truth once any filter param is
  // present — a shared link is never silently overridden by local storage.
  interface StoredIssueFilters {
    status: Filter;
    sort: Sort;
    level: Level;
    environment: string;
    release: string;
  }

  const FILTER_PARAMS = ['status', 'sort', 'level', 'environment', 'release'] as const;

  function filtersStorageKey(id: string): string {
    return `soika:issues:${id}:filters`;
  }

  $effect(() => {
    const id = projectId;
    const url = $page.url;
    if (FILTER_PARAMS.some((key) => url.searchParams.has(key))) return;

    const stored = readJson<Partial<StoredIssueFilters>>(filtersStorageKey(id));
    if (!stored) return;

    const status: Filter =
      stored.status && filters.some((f) => f.value === stored.status)
        ? stored.status
        : 'unresolved';
    const sort: Sort =
      stored.sort && sorts.some((s) => s.value === stored.sort) ? stored.sort : 'last_seen';
    const level: Level =
      stored.level && levels.some((l) => l.value === stored.level) ? stored.level : 'all';
    const environment = typeof stored.environment === 'string' ? stored.environment : '';
    const release = typeof stored.release === 'string' ? stored.release : '';

    if (
      status === 'unresolved' &&
      sort === 'last_seen' &&
      level === 'all' &&
      environment === '' &&
      release === ''
    ) {
      return;
    }

    const next = new URL(url);
    if (status !== 'unresolved') next.searchParams.set('status', status);
    if (sort !== 'last_seen') next.searchParams.set('sort', sort);
    if (level !== 'all') next.searchParams.set('level', level);
    if (environment !== '') next.searchParams.set('environment', environment);
    if (release !== '') next.searchParams.set('release', release);
    goto(next, { replaceState: true, keepFocus: true, noScroll: true });
  });

  let issues = $state<Issue[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  // Re-fetch only for non-default views (a status/sort/field filter is active).
  // The default view is served straight from the layout seed — opening or
  // returning to the Issues tab fires no request. The seed stays fresh because
  // issue mutations call `invalidateAll()`, which re-runs the layout load.
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

    // Default view: render the pre-loaded layout seed, no request.
    if (isDefaultView) {
      issues = data.issues;
      loading = false;
      loadError = null;
      return;
    }

    loading = true;
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
          loadError = errorMessage(err, $t('failed_to_load_issues'));
          issues = [];
          loading = false;
        }
        reportUnexpected(err);
      });
    return () => {
      cancelled = true;
    };
  });

  // Set or delete a single URL search param, preserving the others. An empty /
  // null value (or a value equal to `clearOn`) removes the param so we never
  // send a blank filter to the API.
  function setParam(key: string, value: string | null, clearOn: string | undefined = undefined) {
    const url = new URL($page.url);
    const trimmed = value?.trim() ?? '';
    if (trimmed === '' || trimmed === clearOn) url.searchParams.delete(key);
    else url.searchParams.set(key, trimmed);
    goto(url, { replaceState: true, keepFocus: true, noScroll: true });

    writeJson<StoredIssueFilters>(filtersStorageKey(projectId), {
      status: (url.searchParams.get('status') as Filter | null) ?? 'unresolved',
      sort: (url.searchParams.get('sort') as Sort | null) ?? 'last_seen',
      level: (url.searchParams.get('level') as Level | null) ?? 'all',
      environment: url.searchParams.get('environment') ?? '',
      release: url.searchParams.get('release') ?? ''
    });
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

  // Severity colour for the level dot: errors read red, warnings amber, the
  // rest stay neutral so a list of errors is not a wall of colour.
  function levelTone(level: string): string {
    if (level === 'fatal' || level === 'error') return 'text-destructive';
    if (level === 'warning') return 'text-amber-600 dark:text-amber-400';
    return 'text-muted-foreground';
  }
</script>

<PageTitle title={data.project.name} />

<div class="space-y-4">
  <div class="flex flex-wrap items-center gap-2">
    <SegmentedControl
      label={$t('status')}
      value={urlStatus}
      onselect={selectFilter}
      options={filters}
    />

    <span class="bg-border mx-1 hidden h-5 w-px sm:block"></span>

    <NativeSelect
      value={urlLevel}
      onchange={(e) => selectLevel(e.currentTarget.value as Level)}
      class={urlLevel !== 'all' ? 'border-primary/50 ring-primary/15 ring-3' : ''}
      aria-label={$t('filter_by_level')}
    >
      {#each levels as l (l.value)}
        <option value={l.value}>{l.label}</option>
      {/each}
    </NativeSelect>

    <Input
      type="text"
      value={urlEnvironment}
      onchange={(e) => setParam('environment', e.currentTarget.value)}
      placeholder={$t('environment')}
      aria-label={$t('filter_by_environment')}
      class={cn('w-36', urlEnvironment !== '' && 'border-primary/50 ring-primary/15 ring-3')}
    />

    <Input
      type="text"
      value={urlRelease}
      onchange={(e) => setParam('release', e.currentTarget.value)}
      placeholder={$t('release')}
      aria-label={$t('filter_by_release')}
      class={cn('w-36', urlRelease !== '' && 'border-primary/50 ring-primary/15 ring-3')}
    />

    <SegmentedControl
      class="sm:ml-auto"
      label={$t('sort_order')}
      value={urlSort}
      onselect={selectSort}
      options={sorts}
    />
  </div>

  {#if loading}
    <Card.Root>
      <Card.Content class="text-muted-foreground py-12 text-center text-sm"
        >{$t('loading_issues')}</Card.Content
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
          <p class="font-medium">{$t('no_issues_here')}</p>
          <p class="text-muted-foreground text-sm">
            {urlStatus === 'unresolved'
              ? $t('no_unresolved_issues_nice_and_quiet')
              : $t('no_issues_match_this_filter')}
          </p>
        </div>
      </Card.Content>
    </Card.Root>
  {:else}
    <div class="bg-card overflow-x-auto rounded-xl border">
      <div class="min-w-[36rem]">
        <div
          class="border-hairline text-muted-foreground grid h-10 grid-cols-[minmax(0,1fr)_6.5rem_5.5rem_6rem] items-center gap-4 border-b px-5 font-mono text-[11px] tracking-[0.08em] uppercase"
        >
          <span>{$t('issues')}</span>
          <span>{$t('status')}</span>
          <span class="text-right">{$t('events')}</span>
          <span class="text-right">{$t('last_seen')}</span>
        </div>
        {#each issues as issue (issue.id)}
          <a
            href={`/projects/${projectId}/issues/${issue.id}`}
            class="border-hairline hover:bg-accent/60 grid grid-cols-[minmax(0,1fr)_6.5rem_5.5rem_6rem] items-center gap-4 border-b px-5 py-3 transition-colors last:border-b-0"
          >
            <div class="min-w-0">
              <div class="truncate font-medium tracking-[-0.005em]">{issue.title}</div>
              <div class="text-muted-foreground mt-0.5 flex min-w-0 items-center gap-2 text-xs">
                {#if issue.level}
                  <span
                    class={cn(
                      'inline-flex shrink-0 items-center gap-1.5 font-mono text-[11px] tracking-[0.04em]',
                      levelTone(issue.level)
                    )}
                  >
                    <span class="size-1.5 rounded-full bg-current"></span>
                    {issue.level}
                  </span>
                {/if}
                {#if issue.culprit}
                  <span class="truncate font-mono">{issue.culprit}</span>
                {/if}
              </div>
            </div>

            <div><IssueStatusBadge status={issue.status} /></div>

            <div class="text-right text-[15px] font-medium tracking-[-0.01em] tabular-nums">
              {formatCount(issue.event_count, $locale)}
            </div>

            <div class="text-muted-foreground text-right text-[13px] tabular-nums">
              {formatRelative(issue.last_seen, $locale)}
            </div>
          </a>
        {/each}
      </div>
    </div>
    <p class="text-muted-foreground text-xs">
      {$t('issue_count', { values: { count: issues.length } })}
    </p>
  {/if}
</div>
