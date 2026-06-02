<script lang="ts">
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { invites, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { Badge } from '$lib/components/ui/badge';
  import { toast } from '$lib/components/ui/sonner';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  // For a new email the invite itself authorizes registration, so we
  // collect a password (+ name) when the preview says registration is required.
  // Seed the editable field from the (one-time) loaded preview email.
  let email = $state(untrack(() => data.preview?.email ?? ''));
  let displayName = $state('');
  let password = $state('');
  let submitting = $state(false);

  const requiresRegistration = $derived(data.preview?.requires_registration ?? false);

  async function accept(event: SubmitEvent) {
    event.preventDefault();
    if (!data.preview) return;
    submitting = true;
    try {
      const user = await invites.accept(data.token, {
        email: email || undefined,
        password: password || undefined,
        display_name: displayName || undefined
      });
      authStore.set(user);
      toast.success('Invite accepted');
      await goto(`/projects/${data.preview.project_id}`);
    } catch (err) {
      toast.error(errorMessage(err, 'Could not accept invite'));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    {#if data.error || !data.preview}
      <Card.Header>
        <Card.Title>Invite unavailable</Card.Title>
        <Card.Description>{data.error ?? 'This invite is no longer valid.'}</Card.Description>
      </Card.Header>
      <Card.Footer>
        <Button href="/login" variant="outline">Go to sign in</Button>
      </Card.Footer>
    {:else}
      <Card.Header>
        <Card.Title>You're invited</Card.Title>
        <Card.Description>
          Join with the role
          <Badge variant="secondary">{data.preview.role}</Badge>
        </Card.Description>
      </Card.Header>
      <form onsubmit={accept} class="contents">
        <Card.Content class="space-y-4">
          <div class="space-y-2">
            <label for="email" class="text-sm font-medium">Email</label>
            <Input id="email" type="email" bind:value={email} required />
          </div>
          {#if requiresRegistration}
            <div class="space-y-2">
              <label for="name" class="text-sm font-medium">Display name</label>
              <Input id="name" bind:value={displayName} required />
            </div>
          {/if}
          <div class="space-y-2">
            <label for="password" class="text-sm font-medium">Password</label>
            <Input
              id="password"
              type="password"
              autocomplete={requiresRegistration ? 'new-password' : 'current-password'}
              bind:value={password}
              required
            />
          </div>
        </Card.Content>
        <Card.Footer>
          <Button type="submit" class="w-full" disabled={submitting}>
            {submitting ? 'Joining…' : 'Accept invite'}
          </Button>
        </Card.Footer>
      </form>
    {/if}
  </Card.Root>
</div>
