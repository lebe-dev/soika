<script lang="ts">
  import '../app.css';
  import type { Snippet } from 'svelte';
  import { ModeWatcher } from 'mode-watcher';
  import { Toaster } from '$lib/components/ui/sonner';
  import { authStore } from '$lib/stores/auth.svelte';
  import type { LayoutData } from './$types';

  let { data, children }: { data: LayoutData; children: Snippet } = $props();

  // Seed the reactive auth cache from the root load (runs once on navigation
  // into the app). Individual flows (login/logout) update the store directly.
  $effect(() => {
    authStore.set(data.user);
  });
</script>

<ModeWatcher defaultMode="dark" />
<Toaster richColors closeButton />

{@render children()}
