<script lang="ts">
  import { goto } from '$app/navigation';
  import { auth, ApiError, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';

  // Register — only succeeds when the instance has `allow_signup` enabled (§10.1).
  // There is no public endpoint to read the flag before submitting (GET /settings
  // is admin-only), so a disabled instance is detected from the 403 the register
  // endpoint returns; we then switch the page into a persistent disabled state.
  let email = $state('');
  let displayName = $state('');
  let password = $state('');
  let submitting = $state(false);
  let signupDisabled = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    try {
      const user = await auth.register({ email, password, display_name: displayName });
      authStore.set(user);
      toast.success('Account created');
      await goto('/');
    } catch (err) {
      if (err instanceof ApiError && err.isForbidden) {
        signupDisabled = true;
        return;
      }
      toast.error(errorMessage(err, 'Registration failed'));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    {#if signupDisabled}
      <Card.Header>
        <Card.Title>Sign-up is disabled</Card.Title>
        <Card.Description>
          Public registration is turned off for this instance. Ask an administrator for an invite,
          or sign in if you already have an account.
        </Card.Description>
      </Card.Header>
      <Card.Footer>
        <Button href="/login" variant="outline" class="w-full">Go to sign in</Button>
      </Card.Footer>
    {:else}
      <Card.Header>
        <Card.Title>Create your account</Card.Title>
        <Card.Description>Registration must be enabled by an administrator.</Card.Description>
      </Card.Header>
      <form onsubmit={submit} class="contents">
        <Card.Content class="space-y-4">
          <div class="space-y-2">
            <label for="name" class="text-sm font-medium">Display name</label>
            <Input id="name" autocomplete="name" bind:value={displayName} required />
          </div>
          <div class="space-y-2">
            <label for="email" class="text-sm font-medium">Email</label>
            <Input id="email" type="email" autocomplete="email" bind:value={email} required />
          </div>
          <div class="space-y-2">
            <label for="password" class="text-sm font-medium">Password</label>
            <Input
              id="password"
              type="password"
              autocomplete="new-password"
              bind:value={password}
              minlength={8}
              required
            />
          </div>
        </Card.Content>
        <Card.Footer class="flex-col items-stretch gap-3">
          <Button type="submit" disabled={submitting}>
            {submitting ? 'Creating…' : 'Create account'}
          </Button>
          <p class="text-muted-foreground text-center text-sm">
            Already have an account?
            <a href="/login" class="text-foreground underline-offset-4 hover:underline">Sign in</a>
          </p>
        </Card.Footer>
      </form>
    {/if}
  </Card.Root>
</div>
