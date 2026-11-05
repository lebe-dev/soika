<script lang="ts">
  // "‹ Parent" link above an entity header. One style for every detail page.
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import { cn } from '$lib/utils';

  let {
    href,
    label,
    highlighted = false
  }: { href: string; label: string; highlighted?: boolean } = $props();
</script>

<a
  {href}
  class={cn(
    'inline-flex min-h-7 items-center gap-1 text-[13px] transition-colors',
    highlighted
      ? 'text-primary hover:text-primary/80'
      : 'text-muted-foreground hover:text-foreground'
  )}
>
  <ChevronLeft class="size-3.5" />
  {label}
</a>
