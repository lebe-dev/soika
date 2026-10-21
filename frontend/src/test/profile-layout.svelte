<script lang="ts">
  // Mirror the root layout's auth-cache effect while exercising the real
  // profile component: a saved language must survive the next effect flush.
  import type { User } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import Profile from '../routes/(app)/profile/+page.svelte';
  let { user }: { user: User } = $props();
  $effect(() => authStore.set(user));
</script>

<Profile />
