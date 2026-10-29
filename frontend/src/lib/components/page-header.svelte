<script lang="ts">
  // Header for a top-level list page (Projects, Teams, Admin): title, a quiet
  // one-line description, and primary actions on the right.
  import type { Snippet } from 'svelte';

  let { title, description, actions }: { title: string; description?: Snippet; actions?: Snippet } =
    $props();
</script>

<div class="flex flex-wrap items-end justify-between gap-4">
  <div class="min-w-0">
    <h1 class="text-[1.75rem] leading-tight font-semibold tracking-[-0.025em]">{title}</h1>
    {#if description}
      <p class="text-muted-foreground mt-1.5 text-sm">{@render description()}</p>
    {/if}
  </div>
  {#if actions}
    <div class="flex shrink-0 items-center gap-2">
      {@render actions()}
    </div>
  {/if}
</div>
