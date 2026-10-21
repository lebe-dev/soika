<script lang="ts">
  import { t } from '$lib/i18n';
  import { Button } from '$lib/components/ui/button';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { toast } from '$lib/components/ui/sonner';
  import { errorMessage } from '$lib/api';
  import StackFrame from '$lib/components/stack-frame.svelte';
  import { type Stacktrace, framesForDisplay, isInApp, stacktraceToText } from '$lib/stacktrace';
  import ChevronsUpDown from '@lucide/svelte/icons/chevrons-up-down';
  import Layers from '@lucide/svelte/icons/layers';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';
  import Copy from '@lucide/svelte/icons/copy';

  // Stacktrace viewer renders the most relevant (in-app) frame first,
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

  // Copy the full stacktrace to the clipboard as plain text.
  async function copyAsText() {
    try {
      await navigator.clipboard.writeText(stacktraceToText(stacktrace));
      toast.success($t('copied_stacktrace'));
    } catch (err) {
      toast.error(errorMessage(err, $t('could_not_copy_to_clipboard')));
    }
  }
</script>

<div class="overflow-hidden rounded-lg border">
  <div class="bg-muted/40 flex items-center justify-between gap-2 border-b px-3 py-2">
    <div class="flex items-center gap-2 text-sm font-medium">
      <Layers class="text-muted-foreground size-4" />
      {$t('stacktrace')}
      <span class="text-muted-foreground text-xs font-normal">
        ({$t('frame_count', { values: { count: frames.length } })})
      </span>
    </div>
    <div class="flex items-center gap-1">
      {#if hasInApp && systemCount > 0}
        <Button
          variant="ghost"
          size="sm"
          class="h-7 gap-1.5 text-xs"
          onclick={() => (showSystem = !showSystem)}
        >
          <ChevronsUpDown class="size-3.5" />
          {$t(showSystem ? 'hide_system_frames' : 'show_system_frames', {
            values: { count: systemCount }
          })}
        </Button>
      {/if}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          <Button variant="ghost" size="icon" class="size-7" aria-label={$t('stacktrace_actions')}>
            <EllipsisVertical class="size-3.5" />
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="w-40">
          <DropdownMenu.Item onclick={copyAsText} class="text-xs">
            <Copy class="size-3.5" />
            {$t('copy')}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </div>
  </div>

  <div>
    {#each visibleFrames as frame, i (`${i}-${frame.function ?? ''}-${frame.lineno ?? ''}`)}
      <StackFrame {frame} expanded={isFirst(i)} crashing={isFirst(i)} />
    {/each}
  </div>
</div>
