<script lang="ts">
  import { untrack } from 'svelte';
  import { goto, invalidateAll } from '$app/navigation';
  import { issues as issuesApi, errorMessage, type Issue, type SoikaEvent } from '$lib/api';
  import * as Card from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { toast } from '$lib/components/ui/sonner';
  import IssueStatusBadge from '$lib/components/issue-status-badge.svelte';
  import StacktraceViewer from '$lib/components/stacktrace-viewer.svelte';
  import EventContext from '$lib/components/event-context.svelte';
  import { parseEvent } from '$lib/stacktrace';
  import { formatCount, formatDateTime, formatRelative } from '$lib/format';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import CheckCircle2 from '@lucide/svelte/icons/circle-check-big';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import VolumeX from '@lucide/svelte/icons/volume-x';

  // Issue detail with stacktrace viewer, event navigation, and resolve/mute
  // actions. Members and admins may both resolve/mute.
  let { data }: { data: PageData } = $props();

  const projectId = $derived(data.issue.project_id);

  // Local, reactive copy of the issue so resolve/mute update the UI in place.
  let issue = $state<Issue>(untrack(() => data.issue));
  $effect(() => {
    issue = data.issue;
  });

  // Events list (newest first). Seed the carousel with the latest event; fall
  // back to the issue's latest_event when the events list is empty.
  const events = $derived<SoikaEvent[]>(
    data.events.length > 0 ? data.events : data.issue.latest_event ? [data.issue.latest_event] : []
  );

  let eventIndex = $state(0);
  $effect(() => {
    // Reset to the newest event when navigating between issues.
    data.issue;
    eventIndex = 0;
  });

  const currentEvent = $derived<SoikaEvent | undefined>(events[eventIndex]);
  const parsed = $derived(currentEvent ? parseEvent(currentEvent.payload) : undefined);

  let acting = $state(false);

  async function run(action: 'resolve' | 'mute' | 'unresolve') {
    acting = true;
    try {
      const updated = await issuesApi[action](issue.id);
      issue = { ...issue, ...updated };
      const labels = { resolve: 'resolved', mute: 'muted', unresolve: 'reopened' } as const;
      toast.success(`Issue ${labels[action]}`);
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Action failed'));
    } finally {
      acting = false;
    }
  }

  // Merge / edit fingerprint. Opening seeds the draft from the
  // current fingerprint; saving may MERGE into another issue, in which case the
  // surviving issue's id differs and we navigate to it.
  let editingFp = $state(false);
  let fpDraft = $state('');

  function openFingerprintEditor() {
    fpDraft = issue.fingerprint;
    editingFp = true;
  }

  async function saveFingerprint() {
    const next = fpDraft.trim();
    if (!next || next === issue.fingerprint) {
      editingFp = false;
      return;
    }
    acting = true;
    try {
      const updated = await issuesApi.setFingerprint(issue.id, next);
      toast.success('Fingerprint updated');
      editingFp = false;
      await invalidateAll();
      if (updated.id !== issue.id) {
        await goto(`/projects/${projectId}/issues/${updated.id}`);
        return;
      }
      issue = { ...issue, ...updated };
    } catch (err) {
      toast.error(errorMessage(err, 'Could not update fingerprint'));
    } finally {
      acting = false;
    }
  }

  function prevEvent() {
    if (eventIndex < events.length - 1) eventIndex += 1;
  }
  function nextEvent() {
    if (eventIndex > 0) eventIndex -= 1;
  }
</script>

<PageTitle title={issue.title} />

<div class="space-y-6">
  <a
    href={`/projects/${projectId}`}
    class="text-primary hover:text-primary/80 inline-flex items-center gap-1.5 text-sm transition-colors"
  >
    <ArrowLeft class="size-4" />
    Back to issues
  </a>

  <!-- Header: title, status, counters, actions -->
  <div class="flex flex-wrap items-start justify-between gap-4">
    <div class="min-w-0 space-y-1">
      <div class="flex items-center gap-2">
        {#if issue.level}
          <span class="text-muted-foreground text-xs font-semibold tracking-wide uppercase"
            >{issue.level}</span
          >
        {/if}
        <IssueStatusBadge status={issue.status} />
      </div>
      <h1 class="text-xl font-semibold tracking-tight break-words">{issue.title}</h1>
      {#if issue.culprit}
        <p class="text-muted-foreground font-mono text-sm break-all">{issue.culprit}</p>
      {/if}
    </div>

    <div class="flex shrink-0 gap-2">
      {#if issue.status === 'resolved'}
        <Button variant="outline" size="sm" disabled={acting} onclick={() => run('unresolve')}>
          <RotateCcw class="size-4" />
          Reopen
        </Button>
      {:else}
        <Button variant="outline" size="sm" disabled={acting} onclick={() => run('resolve')}>
          <CheckCircle2 class="size-4" />
          Resolve
        </Button>
      {/if}

      {#if issue.status === 'muted'}
        <Button variant="outline" size="sm" disabled={acting} onclick={() => run('unresolve')}>
          <VolumeX class="size-4" />
          Unmute
        </Button>
      {:else}
        <Button variant="outline" size="sm" disabled={acting} onclick={() => run('mute')}>
          <VolumeX class="size-4" />
          Mute
        </Button>
      {/if}
    </div>
  </div>

  <!-- Stats -->
  <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
    <Card.Root>
      <Card.Content class="p-4">
        <div class="text-muted-foreground text-xs tracking-wide uppercase">Events</div>
        <div class="text-lg font-semibold tabular-nums">{formatCount(issue.event_count)}</div>
      </Card.Content>
    </Card.Root>
    <Card.Root>
      <Card.Content class="p-4">
        <div class="text-muted-foreground text-xs tracking-wide uppercase">First seen</div>
        <div class="text-sm font-medium" title={formatDateTime(issue.first_seen)}>
          {formatRelative(issue.first_seen)}
        </div>
      </Card.Content>
    </Card.Root>
    <Card.Root>
      <Card.Content class="p-4">
        <div class="text-muted-foreground text-xs tracking-wide uppercase">Last seen</div>
        <div class="text-sm font-medium" title={formatDateTime(issue.last_seen)}>
          {formatRelative(issue.last_seen)}
        </div>
      </Card.Content>
    </Card.Root>
    <Card.Root>
      <Card.Content class="space-y-2 p-4">
        <div class="text-muted-foreground text-xs tracking-wide uppercase">Fingerprint</div>
        {#if editingFp}
          <div class="flex items-center gap-1.5">
            <Input
              class="h-7 font-mono text-xs"
              bind:value={fpDraft}
              disabled={acting}
              aria-label="New fingerprint"
              onkeydown={(e) => e.key === 'Enter' && saveFingerprint()}
            />
            <Button size="sm" class="h-7" disabled={acting} onclick={saveFingerprint}>Save</Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-7"
              disabled={acting}
              onclick={() => (editingFp = false)}>Cancel</Button
            >
          </div>
          <p class="text-muted-foreground text-xs">
            Setting a fingerprint already used by another issue merges them.
          </p>
        {:else}
          <div class="truncate font-mono text-xs" title={issue.fingerprint}>
            {issue.fingerprint}
          </div>
          <Button
            variant="outline"
            size="sm"
            class="h-7"
            disabled={acting}
            onclick={openFingerprintEditor}>Merge / edit</Button
          >
        {/if}
      </Card.Content>
    </Card.Root>
  </div>

  {#if currentEvent}
    <!-- Event navigation -->
    <div class="bg-muted/30 flex items-center justify-between gap-2 rounded-lg border px-3 py-2">
      <div class="min-w-0 text-sm">
        <span class="text-muted-foreground">Event</span>
        <span class="font-medium">{events.length - eventIndex}</span>
        <span class="text-muted-foreground">of {events.length}</span>
        <span class="text-muted-foreground ml-2 font-mono text-xs" title={currentEvent.event_id}>
          {currentEvent.event_id.slice(0, 8)}
        </span>
        <span
          class="text-muted-foreground ml-2 text-xs"
          title={formatDateTime(currentEvent.received_at)}
        >
          · {formatRelative(currentEvent.received_at)}
        </span>
      </div>
      <div class="flex shrink-0 items-center gap-1">
        <!-- "Older" goes toward higher index (events are newest-first). -->
        <Button
          variant="ghost"
          size="icon"
          class="size-8"
          disabled={eventIndex >= events.length - 1}
          onclick={prevEvent}
          aria-label="Older event"
        >
          <ChevronLeft class="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          class="size-8"
          disabled={eventIndex <= 0}
          onclick={nextEvent}
          aria-label="Newer event"
        >
          <ChevronRight class="size-4" />
        </Button>
      </div>
    </div>

    <!-- Exception summary -->
    {#if parsed?.exceptionType || parsed?.exceptionValue}
      <Card.Root>
        <Card.Content class="space-y-1 p-4">
          {#if parsed.exceptionType}
            <div class="font-mono text-sm font-semibold">{parsed.exceptionType}</div>
          {/if}
          {#if parsed.exceptionValue}
            <p class="text-muted-foreground font-mono text-sm break-words">
              {parsed.exceptionValue}
            </p>
          {/if}
        </Card.Content>
      </Card.Root>
    {:else if parsed?.message}
      <Card.Root>
        <Card.Content class="p-4">
          <p class="text-sm break-words">{parsed.message}</p>
        </Card.Content>
      </Card.Root>
    {/if}

    <!-- Event context: browser/OS/IP, user, tags, request, contexts -->
    <EventContext payload={currentEvent.payload} />

    <!-- Stacktrace viewer -->
    {#if parsed?.stacktrace}
      <StacktraceViewer stacktrace={parsed.stacktrace} />
    {:else}
      <Card.Root>
        <Card.Content class="text-muted-foreground py-8 text-center text-sm">
          No stacktrace was reported for this event.
        </Card.Content>
      </Card.Root>
    {/if}
  {:else}
    <Card.Root>
      <Card.Content class="text-muted-foreground py-12 text-center text-sm">
        No events are available for this issue. Older events may have been pruned by retention.
      </Card.Content>
    </Card.Root>
  {/if}
</div>
