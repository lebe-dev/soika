<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { auth, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';

  let email = $state('');
  let password = $state('');
  let submitting = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    try {
      const user = await auth.login({ email, password });
      authStore.set(user);
      const next = $page.url.searchParams.get('next');
      await goto(next && next.startsWith('/') ? next : '/');
    } catch (err) {
      toast.error(errorMessage(err, 'Login failed'));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    <Card.Header>
      <Card.Title>Sign in to soika</Card.Title>
      <Card.Description>Enter your credentials to continue.</Card.Description>
    </Card.Header>
    <form onsubmit={submit}>
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <label for="email" class="text-sm font-medium">Email</label>
          <Input id="email" type="email" autocomplete="email" bind:value={email} required />
        </div>
        <div class="space-y-2">
          <label for="password" class="text-sm font-medium">Password</label>
          <Input
            id="password"
            type="password"
            autocomplete="current-password"
            bind:value={password}
            required
          />
        </div>
      </Card.Content>
      <Card.Footer class="flex-col items-stretch gap-3">
        <Button type="submit" disabled={submitting}>
          {submitting ? 'Signing in…' : 'Sign in'}
        </Button>
        <p class="text-muted-foreground text-center text-sm">
          No account?
          <a href="/register" class="text-foreground underline-offset-4 hover:underline">
            Register
          </a>
        </p>
      </Card.Footer>
    </form>
  </Card.Root>
</div>
