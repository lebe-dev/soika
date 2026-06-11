<script lang="ts">
  import '../app.css';
  import type { Snippet } from 'svelte';
  import { ModeWatcher } from 'mode-watcher';
  import { Toaster } from '$lib/components/ui/sonner';
  import { authStore } from '$lib/stores/auth.svelte';
  import { initSentry, setSentryUser } from '$lib/sentry';
  import * as Sentry from '@sentry/svelte';
  import { afterNavigate } from '$app/navigation';
  import type { LayoutData } from './$types';

  let { data, children }: { data: LayoutData; children: Snippet } = $props();

  // Seed the reactive auth cache from the root load (runs once on navigation
  // into the app). Individual flows (login/logout) update the store directly.
  $effect(() => {
    authStore.set(data.user);
  });

  // Initialize Sentry once the authenticated telemetry config is available (the
  // DSN is only served to signed-in users). Idempotent; a no-op when disabled.
  // After init, attach the current user so events carry { id, email }; this
  // clears the scope on logout (when the cached user becomes null).
  $effect(() => {
    initSentry(data.telemetry);
    setSentryUser(authStore.user);
  });

  // Tag every event with the active route so error reports carry their origin.
  afterNavigate((nav) => {
    const route = nav.to?.route.id ?? nav.to?.url.pathname ?? 'unknown';
    Sentry.getCurrentScope().setTag('route', route);
  });
</script>

<ModeWatcher defaultMode="dark" />
<Toaster richColors closeButton />

{@render children()}
