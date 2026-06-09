<script lang="ts">
  import { untrack } from 'svelte';
  import { cn } from '$lib/utils';
  import { type Frame, frameFile, frameLocation, hasSourceContext, isInApp } from '$lib/stacktrace';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';

  // A single stack frame row with collapsible source context. Source
  // lines render exactly as the SDK reported them — no demangling / source maps.
  let {
    frame,
    expanded = false,
    crashing = false
  }: { frame: Frame; expanded?: boolean; crashing?: boolean } = $props();

  // Seed the open state from the initial `expanded` prop only; toggling is local.
  let open = $state(untrack(() => expanded));

  const inApp = $derived(isInApp(frame));
  const location = $derived(frameLocation(frame) ?? '<unknown>');
  const file = $derived(frameFile(frame));
  const showContext = $derived(hasSourceContext(frame));

  // Build the source-context viewport: pre lines, the highlighted context line,
  // then post lines — with correct 1-based line numbers when `lineno` is known.
  type SourceLine = { no: number | null; text: string; highlight: boolean };
  const sourceLines = $derived.by((): SourceLine[] => {
    if (!showContext) return [];
    const lines: SourceLine[] = [];
    const ctxLineNo = frame.lineno ?? null;
    const preStart = ctxLineNo !== null ? ctxLineNo - frame.pre_context.length : null;
    frame.pre_context.forEach((text, i) => {
      lines.push({ no: preStart !== null ? preStart + i : null, text, highlight: false });
    });
    if (frame.context_line !== undefined) {
      lines.push({ no: ctxLineNo, text: frame.context_line, highlight: true });
    }
    const postStart = ctxLineNo !== null ? ctxLineNo + 1 : null;
    frame.post_context.forEach((text, i) => {
      lines.push({ no: postStart !== null ? postStart + i : null, text, highlight: false });
    });
    return lines;
  });

  function toggle() {
    if (showContext) open = !open;
  }
</script>

<div
  class={cn(
    'border-b last:border-b-0',
    inApp ? 'bg-background' : 'bg-muted/30',
    crashing && 'border-l-primary bg-primary/[0.03] border-l-2'
  )}
>
  <button
    type="button"
    class={cn(
      'flex w-full items-center gap-2 px-3 py-2 text-left text-sm',
      showContext ? 'hover:bg-accent/50 cursor-pointer' : 'cursor-default'
    )}
    onclick={toggle}
    aria-expanded={showContext ? open : undefined}
  >
    {#if showContext}
      <ChevronRight
        class={cn(
          'text-muted-foreground size-4 shrink-0 transition-transform',
          open && 'rotate-90'
        )}
      />
    {:else}
      <span class="size-4 shrink-0"></span>
    {/if}

    <span class="min-w-0 flex-1 truncate font-mono">
      <span class={cn('font-medium', !inApp && 'text-muted-foreground')}>{location}</span>
      {#if file}
        <span class="text-muted-foreground">
          &nbsp;·&nbsp;{file}{#if frame.lineno !== undefined}:{frame.lineno}{#if frame.colno !== undefined}:{frame.colno}{/if}{/if}
        </span>
      {/if}
    </span>

    {#if inApp}
      <span
        class="bg-primary/10 text-primary shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium tracking-wide uppercase"
      >
        in-app
      </span>
    {/if}
  </button>

  {#if showContext && open}
    <div class="bg-muted/20 overflow-x-auto border-t font-mono text-xs leading-relaxed">
      {#each sourceLines as line (`${line.no}-${line.text}`)}
        <div class={cn('flex whitespace-pre', line.highlight && 'bg-destructive/10')}>
          <span
            class="text-muted-foreground w-12 shrink-0 border-r px-2 py-0.5 text-right select-none"
          >
            {line.no ?? ''}
          </span>
          <span class={cn('px-3 py-0.5', line.highlight && 'text-foreground font-medium')}
            >{line.text || ' '}</span
          >
        </div>
      {/each}
    </div>
  {/if}
</div>
