<script lang="ts">
  // "‹ Parent" link above an entity header. One style for every detail page.
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';

  let { href, label }: { href: string; label: string } = $props();
</script>

<a
  {href}
  class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1 text-sm transition-colors"
>
  <ChevronLeft class="size-4" />
  {label}
</a>
