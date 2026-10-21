<script lang="ts">
  // Header for a top-level list page (Projects, Teams, Admin): title, a quiet
  // one-line description, and primary actions on the right.
  import type { Snippet } from 'svelte';

  let { title, description, actions }: { title: string; description?: Snippet; actions?: Snippet } =
    $props();
</script>

<div class="flex flex-wrap items-end justify-between gap-4">
  <div class="min-w-0">
    <h1 class="text-2xl font-semibold tracking-tight">{title}</h1>
    {#if description}
      <p class="text-muted-foreground mt-1 text-sm">{@render description()}</p>
    {/if}
  </div>
  {#if actions}
    <div class="flex shrink-0 items-center gap-2">
      {@render actions()}
    </div>
  {/if}
</div>
