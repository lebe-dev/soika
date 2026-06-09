<script lang="ts">
  import { untrack } from 'svelte';
  import { goto, invalidateAll } from '$app/navigation';
  import { issues as issuesApi, errorMessage, type Issue, type SoikaEvent } from '$lib/api';
  import * as Card from '$lib/components/ui/card';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { toast } from '$lib/components/ui/sonner';
  import IssueStatusBadge from '$lib/components/issue-status-badge.svelte';
  import StacktraceViewer from '$lib/components/stacktrace-viewer.svelte';
  import EventContext from '$lib/components/event-context.svelte';
  import IssueTimeline from '$lib/components/issue-timeline.svelte';
  import { parseEvent } from '$lib/stacktrace';
  import { formatCount, formatDateTime, formatRelative } from '$lib/format';
  import { severityStyle } from '$lib/severity';
  import { cn } from '$lib/utils';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import CheckCircle2 from '@lucide/svelte/icons/circle-check-big';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import VolumeX from '@lucide/svelte/icons/volume-x';
  import Crosshair from '@lucide/svelte/icons/crosshair';
  import Activity from '@lucide/svelte/icons/activity';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';
  import Copy from '@lucide/svelte/icons/copy';

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

  // Severity-driven accent (rail, level pill, hero tint, accent border).
  const sev = $derived(severityStyle(issue.level));
  const SevIcon = $derived(sev.icon);

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

  // Copy the issue (with the currently shown event) to the clipboard as JSON.
  async function copyAsJson() {
    const json = JSON.stringify({ ...issue, event: currentEvent ?? null }, null, 2);
    try {
      await navigator.clipboard.writeText(json);
      toast.success('Copied issue as JSON');
    } catch (err) {
      toast.error(errorMessage(err, 'Could not copy to clipboard'));
    }
  }
</script>

<PageTitle title={issue.title.length > 60 ? issue.title.slice(0, 60) + '…' : issue.title} />

<div class="reveal space-y-6">
  <a
    href={`/projects/${projectId}`}
    class="text-primary hover:text-primary/80 inline-flex items-center gap-1.5 text-sm transition-colors"
  >
    <ArrowLeft class="size-4" />
    Back to issues
  </a>

  <!-- Hero: severity rail, level, title, culprit, actions -->
  <div class={cn('ring-foreground/10 flex gap-4 overflow-hidden rounded-xl ring-1', sev.tint)}>
    <div class={cn('w-1.5 shrink-0', sev.rail)} aria-hidden="true"></div>
    <div class="min-w-0 flex-1 space-y-3 py-4 pr-4">
      <div class="flex items-start justify-between gap-4">
        <div class="flex flex-wrap items-center gap-2">
          <span
            class={cn(
              'inline-flex h-5 items-center gap-1.5 rounded-[min(var(--radius-md),12px)] border px-2 py-0.5 text-xs font-semibold tracking-wide uppercase',
              sev.pill
            )}
          >
            <SevIcon class="size-3" />
            {sev.label}
          </span>
          <IssueStatusBadge status={issue.status} class="rounded-[min(var(--radius-md),12px)]" />
        </div>

        <div class="flex shrink-0 gap-2">
          {#if issue.status === 'resolved'}
            <Button variant="outline" size="sm" disabled={acting} onclick={() => run('unresolve')}>
              <RotateCcw class="size-4" />
              Reopen
            </Button>
          {:else}
            <Button size="sm" disabled={acting} onclick={() => run('resolve')}>
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

          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              <Button variant="outline" size="sm" class="px-2" aria-label="More actions">
                <EllipsisVertical class="size-4" />
              </Button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="end" class="w-44">
              <DropdownMenu.Item onclick={copyAsJson} class="text-xs">
                <Copy class="size-4" />
                Copy as JSON
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </div>
      </div>

      <div class="space-y-1">
        <h1 class="text-2xl font-semibold tracking-tight break-words">{issue.title}</h1>
        {#if issue.culprit}
          <p class="text-muted-foreground flex items-center gap-1.5 font-mono text-sm break-all">
            <Crosshair class="size-3.5 shrink-0" />
            {issue.culprit}
          </p>
        {/if}
      </div>
    </div>
  </div>

  <!-- Stats: Events hero metric, lifetime timeline, fingerprint -->
  <div class="grid grid-cols-1 gap-3 sm:grid-cols-4">
    <Card.Root class="sm:col-span-1">
      <Card.Content class="flex h-full flex-col justify-between gap-2 p-4">
        <div
          class="text-muted-foreground flex items-center gap-1.5 text-xs tracking-wide uppercase"
        >
          <Activity class="size-3.5" />
          Events
        </div>
        <div class="text-3xl font-semibold tabular-nums">{formatCount(issue.event_count)}</div>
      </Card.Content>
    </Card.Root>
    <Card.Root class="sm:col-span-2">
      <Card.Content class="flex h-full flex-col justify-center p-4">
        <IssueTimeline firstSeen={issue.first_seen} lastSeen={issue.last_seen} />
      </Card.Content>
    </Card.Root>
    <Card.Root class="sm:col-span-1">
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
      <Card.Root class={cn('border-l-2', sev.accentBorder)}>
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

<style>
  /* One orchestrated page-load reveal: each top-level section fades/slides up
     with a short stagger. Disabled under reduced-motion. */
  @keyframes reveal {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  .reveal > :global(*) {
    animation: reveal 0.35s cubic-bezier(0.2, 0.6, 0.2, 1) both;
  }
  .reveal > :global(*:nth-child(1)) {
    animation-delay: 0ms;
  }
  .reveal > :global(*:nth-child(2)) {
    animation-delay: 40ms;
  }
  .reveal > :global(*:nth-child(3)) {
    animation-delay: 80ms;
  }
  .reveal > :global(*:nth-child(4)) {
    animation-delay: 120ms;
  }
  .reveal > :global(*:nth-child(5)) {
    animation-delay: 160ms;
  }
  .reveal > :global(*:nth-child(n + 6)) {
    animation-delay: 200ms;
  }
  @media (prefers-reduced-motion: reduce) {
    .reveal > :global(*) {
      animation: none;
    }
  }
</style>
