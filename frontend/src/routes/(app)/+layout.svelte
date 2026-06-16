<script lang="ts">
  import type { Snippet } from 'svelte';
  import AppNav from '$lib/components/app-nav.svelte';
  import type { LayoutData } from './$types';

  let { data, children }: { data: LayoutData; children: Snippet } = $props();
</script>

<div class="flex min-h-screen flex-col">
  <AppNav user={data.user} pendingApprovals={data.pendingApprovals} />
  <main class="container flex-1 py-8">
    {@render children()}
  </main>
</div>
