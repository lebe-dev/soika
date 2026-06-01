<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import StackFrame from '$lib/components/stack-frame.svelte';
  import { type Stacktrace, framesForDisplay, isInApp } from '$lib/stacktrace';
  import ChevronsUpDown from '@lucide/svelte/icons/chevrons-up-down';
  import Layers from '@lucide/svelte/icons/layers';

  // Stacktrace viewer (MVP §6): renders the most relevant (in-app) frame first,
  // with expand/collapse for the full trace and per-frame source context. By
  // default only application frames are shown; system/library frames are folded
  // away behind a toggle, mirroring Sentry's default "in-app only" view.
  let { stacktrace }: { stacktrace: Stacktrace } = $props();

  // Display order: crashing frame first (we reverse the SDK's oldest-first list).
  const frames = $derived(framesForDisplay(stacktrace));
  const hasInApp = $derived(frames.some(isInApp));
  const systemCount = $derived(frames.filter((f) => !isInApp(f)).length);

  // When in-app frames exist, default to hiding system frames; otherwise show all.
  let showSystem = $state(false);
  const allInAppShown = $derived(hasInApp ? showSystem : true);

  const visibleFrames = $derived(allInAppShown ? frames : frames.filter(isInApp));

  // The crashing (most relevant) frame is first in display order — auto-expand
  // its source context so the user lands on what matters.
  function isFirst(index: number): boolean {
    return index === 0;
  }
</script>

<div class="overflow-hidden rounded-lg border">
  <div class="bg-muted/40 flex items-center justify-between gap-2 border-b px-3 py-2">
    <div class="flex items-center gap-2 text-sm font-medium">
      <Layers class="text-muted-foreground size-4" />
      Stacktrace
      <span class="text-muted-foreground text-xs font-normal">
        ({frames.length}
        {frames.length === 1 ? 'frame' : 'frames'})
      </span>
    </div>
    {#if hasInApp && systemCount > 0}
      <Button
        variant="ghost"
        size="sm"
        class="h-7 gap-1.5 text-xs"
        onclick={() => (showSystem = !showSystem)}
      >
        <ChevronsUpDown class="size-3.5" />
        {showSystem ? 'Hide' : 'Show'} system frames ({systemCount})
      </Button>
    {/if}
  </div>

  <div>
    {#each visibleFrames as frame, i (`${i}-${frame.function ?? ''}-${frame.lineno ?? ''}`)}
      <StackFrame {frame} expanded={isFirst(i)} />
    {/each}
  </div>
</div>
