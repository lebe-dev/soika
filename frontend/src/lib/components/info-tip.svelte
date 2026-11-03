<script lang="ts">
  import * as Tooltip from '$lib/components/ui/tooltip';
  import Info from '@lucide/svelte/icons/info';
  import type { Snippet } from 'svelte';

  // Small (i) icon that explains a setting or value in a theme-aware tooltip.
  // Pass plain text via `text`, or richer content via `children`.
  let { text, label, children }: { text?: string; label?: string; children?: Snippet } = $props();
</script>

<Tooltip.Provider delayDuration={150}>
  <Tooltip.Root>
    <Tooltip.Trigger
      type="button"
      aria-label={label ?? text}
      class="text-muted-foreground hover:text-foreground focus-visible:ring-primary/30 inline-flex shrink-0 cursor-help items-center rounded-full align-middle transition-colors outline-none focus-visible:ring-2"
    >
      <Info class="size-3.5" />
    </Tooltip.Trigger>
    <Tooltip.Content>
      {#if children}{@render children()}{:else}{text}{/if}
    </Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>
