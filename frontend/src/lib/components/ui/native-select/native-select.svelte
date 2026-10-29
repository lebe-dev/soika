<script lang="ts">
  // Native <select> styled to match <Input>: same height, radius, border, and
  // focus ring, so filters and forms line up wherever they mix the two.
  import type { HTMLSelectAttributes } from 'svelte/elements';
  import { cn, type WithElementRef } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    value = $bindable(),
    class: className,
    children,
    ...restProps
  }: WithElementRef<HTMLSelectAttributes> = $props();
</script>

<select
  bind:this={ref}
  bind:value
  data-slot="native-select"
  class={cn(
    'border-input bg-card focus-visible:border-primary focus-visible:ring-primary/15 dark:[&>option]:bg-popover h-8 rounded-lg border px-2.5 text-base shadow-[0_1px_2px_rgb(20_23_28/0.03)] transition-colors outline-none focus-visible:ring-3 disabled:cursor-not-allowed disabled:opacity-50 md:text-sm',
    className
  )}
  {...restProps}
>
  {@render children?.()}
</select>
