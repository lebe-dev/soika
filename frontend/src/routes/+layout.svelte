<script lang="ts">
  import '../app.css';
  import type { Snippet } from 'svelte';
  import { ModeWatcher } from 'mode-watcher';
  import { Toaster } from '$lib/components/ui/sonner';
  import { authStore } from '$lib/stores/auth.svelte';
  import { initSentry } from '$lib/sentry';
  import type { LayoutData } from './$types';

  let { data, children }: { data: LayoutData; children: Snippet } = $props();

  // Seed the reactive auth cache from the root load (runs once on navigation
  // into the app). Individual flows (login/logout) update the store directly.
  $effect(() => {
    authStore.set(data.user);
  });

  // Initialize Sentry once the authenticated telemetry config is available (the
  // DSN is only served to signed-in users). Idempotent; a no-op when disabled.
  $effect(() => {
    initSentry(data.telemetry);
  });
</script>

<ModeWatcher defaultMode="dark" />
<Toaster richColors closeButton />

{@render children()}
