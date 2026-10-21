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
    'border-input dark:bg-input/30 focus-visible:border-primary focus-visible:ring-primary/30 dark:[&>option]:bg-popover h-8 rounded-lg border bg-transparent px-2.5 text-base transition-colors outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50 md:text-sm',
    className
  )}
  {...restProps}
>
  {@render children?.()}
</select>
