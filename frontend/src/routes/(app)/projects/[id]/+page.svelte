<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { projects, errorMessage, type Issue, type IssueStatus } from '$lib/api';
  import * as Card from '$lib/components/ui/card';
  import IssueStatusBadge from '$lib/components/issue-status-badge.svelte';
  import { cn } from '$lib/utils';
  import { formatCount, formatRelative } from '$lib/format';
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

  let issues = $state<Issue[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  // Re-fetch whenever the project or the URL status filter changes.
  $effect(() => {
    const status = urlStatus;
    const id = projectId;
    loading = true;
    loadError = null;
    const query = status === 'all' ? {} : { status };
    projects
      .issues(id, query)
      .then((res) => {
        issues = res;
      })
      .catch((err) => {
        loadError = errorMessage(err, 'Failed to load issues');
        issues = [];
      })
      .finally(() => {
        loading = false;
      });
  });

  function selectFilter(value: Filter) {
    const url = new URL($page.url);
    if (value === 'unresolved') url.searchParams.delete('status');
    else url.searchParams.set('status', value);
    goto(url, { replaceState: true, keepFocus: true, noScroll: true });
  }
</script>

<div class="space-y-4">
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
