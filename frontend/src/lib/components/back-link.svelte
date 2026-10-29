<script lang="ts">
  // "‹ Parent" link above an entity header. One style for every detail page.
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';

  let { href, label }: { href: string; label: string } = $props();
</script>

<a
  {href}
  class="text-muted-foreground hover:text-foreground inline-flex min-h-7 items-center gap-1 text-[13px] transition-colors"
>
  <ChevronLeft class="size-3.5" />
  {label}
</a>
