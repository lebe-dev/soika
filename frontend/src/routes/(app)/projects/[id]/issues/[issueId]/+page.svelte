<script lang="ts">
  import { t, locale } from '$lib/i18n';
  import { goto, invalidateAll } from '$app/navigation';
  import {
    issues as issuesApi,
    errorMessage,
    type Issue,
    type MuteRequest,
    type SoikaEvent
  } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { toast } from '$lib/components/ui/sonner';
  import IssueStatusBadge from '$lib/components/issue-status-badge.svelte';
  import StacktraceViewer from '$lib/components/stacktrace-viewer.svelte';
  import EventContext from '$lib/components/event-context.svelte';
  import IssueTimeline from '$lib/components/issue-timeline.svelte';
  import TagMuteRuleDialog from '$lib/components/tag-mute-rule-dialog.svelte';
  import { extractContext } from '$lib/event-context';
  import { parseEvent } from '$lib/stacktrace';
  import { formatCount, formatDateTime, formatRelative } from '$lib/format';
  import { severityStyle } from '$lib/severity';
  import { cn } from '$lib/utils';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';
  import BackLink from '$lib/components/back-link.svelte';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import CheckCircle2 from '@lucide/svelte/icons/circle-check-big';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import VolumeX from '@lucide/svelte/icons/volume-x';
  import Crosshair from '@lucide/svelte/icons/crosshair';
  import Activity from '@lucide/svelte/icons/activity';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';
  import Copy from '@lucide/svelte/icons/copy';
  import Tag from '@lucide/svelte/icons/tag';

  // Issue detail with stacktrace viewer, event navigation, and resolve/mute
  // actions. Members and admins may both resolve/mute.
  let { data }: { data: PageData } = $props();

  const timezone = $derived(data.telemetry?.timezone ?? null);
  const projectId = $derived(data.issue.project_id);

  // The issue tracks `data.issue` (refreshed by `load` after navigation or
  // `invalidateAll`). Actions apply an optimistic patch for instant feedback;
  // the patch is keyed to the issue id so it clears once props move on.
  let optimistic = $state<{ id: string; patch: Partial<Issue> } | null>(null);
  const issue = $derived<Issue>(
    optimistic && optimistic.id === data.issue.id
      ? { ...data.issue, ...optimistic.patch }
      : data.issue
  );

  // Events list (newest first). Seed the carousel with the latest event; fall
  // back to the issue's latest_event when the events list is empty.
  const events = $derived<SoikaEvent[]>(
    data.events.length > 0 ? data.events : data.issue.latest_event ? [data.issue.latest_event] : []
  );

  // Carousel position is genuine local nav state (prev/next buttons). Reset it
  // to the newest event whenever we navigate to a different issue.
  let eventIndex = $state(0);
  $effect(() => {
    data.issue.id;
    eventIndex = 0;
  });

  const currentEvent = $derived<SoikaEvent | undefined>(events[eventIndex]);
  const parsed = $derived(currentEvent ? parseEvent(currentEvent.payload) : undefined);

  // Severity-driven accent (rail, level pill, hero tint, accent border).
  const sev = $derived(severityStyle(issue.level));
  const SevIcon = $derived(sev.icon);

  let acting = $state(false);

  async function run(action: 'resolve' | 'unresolve') {
    acting = true;
    try {
      const updated = await issuesApi[action](issue.id);
      optimistic = { id: issue.id, patch: updated };
      toast.success(action === 'resolve' ? $t('issue_resolved') : $t('issue_reopened'));
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('action_failed')));
      reportUnexpected(err);
    } finally {
      acting = false;
    }
  }

  // Mute period options for the "Mute for/until" dropdown. `body` is undefined
  // for an indefinite mute (the bare Mute button); a duration or event-rate
  // otherwise. Auto-unmute is enforced server-side (scheduler / ingestion).
  const DAY = 86_400;
  const muteOptions: { label: string; body?: MuteRequest }[] = $derived([
    { label: $t('mute_day'), body: { duration_seconds: DAY } },
    { label: $t('mute_week'), body: { duration_seconds: 7 * DAY } },
    { label: $t('mute_month'), body: { duration_seconds: 30 * DAY } },
    { label: $t('mute_three_months'), body: { duration_seconds: 90 * DAY } },
    { label: $t('mute_5_hour'), body: { events: 5, window_seconds: 3600 } },
    { label: $t('mute_5_day'), body: { events: 5, window_seconds: DAY } },
    { label: $t('mute_100_day'), body: { events: 100, window_seconds: DAY } }
  ]);

  async function mute(
    body: MuteRequest | undefined = undefined,
    label: string | undefined = undefined
  ) {
    acting = true;
    try {
      const updated = await issuesApi.mute(issue.id, body);
      optimistic = { id: issue.id, patch: updated };
      toast.success(
        label ? $t('issue_muted_period', { values: { period: label } }) : $t('issue_muted')
      );
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('could_not_mute_issue')));
      reportUnexpected(err);
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
      toast.success($t('fingerprint_updated'));
      editingFp = false;
      await invalidateAll();
      if (updated.id !== issue.id) {
        await goto(`/projects/${projectId}/issues/${updated.id}`);
        return;
      }
      optimistic = { id: issue.id, patch: updated };
    } catch (err) {
      toast.error(errorMessage(err, $t('could_not_update_fingerprint')));
      reportUnexpected(err);
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

  // "Mute by tags" — opens a form prefilled with the current event's tags, which
  // creates a project-level rule that suppresses notifications for matching events.
  let muteByTagsOpen = $state(false);
  const currentEventTags = $derived(currentEvent ? extractContext(currentEvent.payload).tags : []);

  // Copy the issue (with the currently shown event) to the clipboard as JSON.
  async function copyAsJson() {
    const json = JSON.stringify({ ...issue, event: currentEvent ?? null }, null, 2);
    try {
      await navigator.clipboard.writeText(json);
      toast.success($t('copied_issue_as_json'));
    } catch (err) {
      toast.error(errorMessage(err, $t('could_not_copy_to_clipboard')));
      reportUnexpected(err);
    }
  }
</script>

<PageTitle title={issue.title.length > 60 ? issue.title.slice(0, 60) + '…' : issue.title} />

<div class="reveal space-y-6">
  <div>
    <BackLink href={`/projects/${projectId}`} label={$t('issues')} />
  </div>

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
            {['fatal', 'error', 'warning', 'info', 'debug', 'unknown'].includes(
              sev.label.toLowerCase()
            )
              ? $t(sev.label.toLowerCase())
              : sev.label}
          </span>
          <IssueStatusBadge status={issue.status} class="rounded-[min(var(--radius-md),12px)]" />
        </div>

        <div class="flex shrink-0 gap-2">
          {#if issue.status === 'resolved'}
            <Button variant="outline" size="sm" disabled={acting} onclick={() => run('unresolve')}>
              <RotateCcw class="size-4" />
              {$t('reopen')}
            </Button>
          {:else}
            <Button size="sm" disabled={acting} onclick={() => run('resolve')}>
              <CheckCircle2 class="size-4" />
              {$t('resolve')}
            </Button>
          {/if}

          {#if issue.status === 'muted'}
            <Button variant="outline" size="sm" disabled={acting} onclick={() => run('unresolve')}>
              <VolumeX class="size-4" />
              {$t('unmute')}
            </Button>
          {:else}
            <!-- Split button: bare "Mute" (forever) + a dropdown of periods. -->
            <div class="flex">
              <Button
                variant="outline"
                size="sm"
                class="rounded-r-none border-r-0"
                disabled={acting}
                onclick={() => mute()}
              >
                <VolumeX class="size-4" />
                {$t('mute')}
              </Button>
              <DropdownMenu.Root>
                <DropdownMenu.Trigger>
                  <Button
                    variant="outline"
                    size="sm"
                    class="rounded-l-none px-2"
                    disabled={acting}
                    aria-label={$t('mute_for_until')}
                  >
                    <ChevronDown class="size-4" />
                  </Button>
                </DropdownMenu.Trigger>
                <DropdownMenu.Content align="end" class="w-52">
                  <DropdownMenu.Label class="text-muted-foreground text-xs">
                    {$t('mute_for_until')}
                  </DropdownMenu.Label>
                  <DropdownMenu.Separator />
                  {#each muteOptions as opt (opt.label)}
                    <DropdownMenu.Item
                      onclick={() => mute(opt.body, opt.label)}
                      class="font-medium"
                    >
                      {opt.label}
                    </DropdownMenu.Item>
                  {/each}
                  <DropdownMenu.Separator />
                  <DropdownMenu.Item onclick={() => (muteByTagsOpen = true)}>
                    <Tag class="size-4" />
                    {$t('mute_by_tags_4deade')}
                  </DropdownMenu.Item>
                </DropdownMenu.Content>
              </DropdownMenu.Root>
            </div>
          {/if}

          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              <Button variant="outline" size="sm" class="px-2" aria-label={$t('more_actions')}>
                <EllipsisVertical class="size-4" />
              </Button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="end" class="w-44">
              <DropdownMenu.Item onclick={copyAsJson}>
                <Copy class="size-4" />
                {$t('copy_as_json')}
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
          {$t('events_c5497b')}
        </div>
        <div class="text-3xl font-semibold tabular-nums">
          {formatCount(issue.event_count, $locale)}
        </div>
      </Card.Content>
    </Card.Root>
    <Card.Root class="sm:col-span-2">
      <Card.Content class="flex h-full flex-col justify-center p-4">
        <IssueTimeline firstSeen={issue.first_seen} lastSeen={issue.last_seen} />
      </Card.Content>
    </Card.Root>
    <Card.Root class="sm:col-span-1">
      <Card.Content class="space-y-2 p-4">
        <div class="text-muted-foreground text-xs tracking-wide uppercase">{$t('fingerprint')}</div>
        {#if editingFp}
          <div class="flex items-center gap-1.5">
            <Input
              class="h-7 font-mono text-xs"
              bind:value={fpDraft}
              disabled={acting}
              aria-label={$t('new_fingerprint')}
              onkeydown={(e) => e.key === 'Enter' && saveFingerprint()}
            />
            <Button size="sm" class="h-7" disabled={acting} onclick={saveFingerprint}
              >{$t('save')}</Button
            >
            <Button
              variant="ghost"
              size="sm"
              class="h-7"
              disabled={acting}
              onclick={() => (editingFp = false)}>{$t('cancel')}</Button
            >
          </div>
          <p class="text-muted-foreground text-xs">
            {$t('setting_a_fingerprint_already_used_by_another_issue_merges_them')}
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
            onclick={openFingerprintEditor}>{$t('merge_edit')}</Button
          >
        {/if}
      </Card.Content>
    </Card.Root>
  </div>

  {#if currentEvent}
    <!-- Event navigation -->
    <div class="bg-muted/30 flex items-center justify-between gap-2 rounded-lg border px-3 py-2">
      <div class="min-w-0 text-sm">
        <span class="text-muted-foreground">{$t('event')}</span>
        <span class="font-medium">{events.length - eventIndex}</span>
        <span class="text-muted-foreground">{$t('of')} {events.length}</span>
        <span class="text-muted-foreground ml-2 font-mono text-xs" title={currentEvent.event_id}>
          {currentEvent.event_id.slice(0, 8)}
        </span>
        <span class="text-muted-foreground ml-2 text-xs">
          · <time
            datetime={currentEvent.received_at}
            title={formatRelative(currentEvent.received_at, $locale)}
            >{formatDateTime(currentEvent.received_at, timezone, $locale)}</time
          >
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
          aria-label={$t('older_event')}
        >
          <ChevronLeft class="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          class="size-8"
          disabled={eventIndex <= 0}
          onclick={nextEvent}
          aria-label={$t('newer_event')}
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
          {$t('no_stacktrace_was_reported_for_this_event')}
        </Card.Content>
      </Card.Root>
    {/if}
  {:else}
    <Card.Root>
      <Card.Content class="text-muted-foreground py-12 text-center text-sm">
        {$t('no_events_are_available_for_this_issue_older_events_may_have_been_pruned_by_rete')}
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<TagMuteRuleDialog bind:open={muteByTagsOpen} {projectId} prefillTags={currentEventTags} />

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
