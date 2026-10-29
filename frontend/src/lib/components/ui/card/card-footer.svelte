<script lang="ts">
  import { cn, type WithElementRef } from '$lib/utils.js';
  import type { HTMLAttributes } from 'svelte/elements';

  let {
    ref = $bindable(null),
    class: className,
    children,
    ...restProps
  }: WithElementRef<HTMLAttributes<HTMLDivElement>> = $props();
</script>

<div
  bind:this={ref}
  data-slot="card-footer"
  class={cn(
    'border-hairline flex items-center gap-3 rounded-b-xl border-t px-5 py-3 group-data-[size=sm]/card:p-3',
    className
  )}
  {...restProps}
>
  {@render children?.()}
</div>
