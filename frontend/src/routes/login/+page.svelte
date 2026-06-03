<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { auth, errorMessage, type AuthConfig } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';
  import PageTitle from '$lib/components/page-title.svelte';

  // Maps callback `?error=<slug>` values to user-facing messages.
  const ERROR_MESSAGES: Record<string, string> = {
    forbidden: 'Your account is not allowed to sign in.',
    auth_failed: 'Sign-in failed. Please try again.',
    invalid_request: 'The sign-in request was invalid. Please try again.',
    sso_failed: 'Single sign-on failed. Please try again.'
  };

  let email = $state('');
  let password = $state('');
  let submitting = $state(false);
  let config = $state<AuthConfig | null>(null);

  // The built-in admin can always reach the password form via `?admin=1` even
  // when SSO replaces password login for everyone else.
  const adminFallback = $derived($page.url.searchParams.get('admin') === '1');
  const showPasswordForm = $derived(
    config === null || config.password_login_enabled || adminFallback
  );
  const showSso = $derived(config?.oauth_enabled ?? false);

  // Open-redirect guard mirroring the backend `validated_next`: only a
  // site-relative path (starts with `/`, not `//`, no backslash — browsers
  // normalize `\` to `/`).
  function safeNext(next: string | null): string | null {
    if (!next || !next.startsWith('/') || next.startsWith('//') || next.includes('\\')) return null;
    return next;
  }

  // Full navigation (not fetch): the next hop is an external provider redirect.
  const ssoHref = $derived.by(() => {
    const next = safeNext($page.url.searchParams.get('next'));
    return next ? `/auth/oidc/login?next=${encodeURIComponent(next)}` : '/auth/oidc/login';
  });

  $effect(() => {
    const slug = $page.url.searchParams.get('error');
    if (slug) toast.error(ERROR_MESSAGES[slug] ?? 'Sign-in failed.');
  });

  $effect(() => {
    void loadConfig();
  });

  async function loadConfig() {
    try {
      config = await auth.config();
    } catch {
      // On failure fall back to showing the password form (config stays null).
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    try {
      const user = await auth.login({ email, password });
      authStore.set(user);
      await goto(safeNext($page.url.searchParams.get('next')) ?? '/');
    } catch (err) {
      toast.error(errorMessage(err, 'Login failed'));
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title="Sign in" />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    <Card.Header>
      <Card.Title>Sign in to soika</Card.Title>
      <Card.Description>
        {showPasswordForm ? 'Enter your credentials to continue.' : 'Continue with single sign-on.'}
      </Card.Description>
    </Card.Header>

    {#if showSso}
      <Card.Content class="space-y-4">
        <Button href={ssoHref} class="w-full" data-sveltekit-reload>
          Войти через {config?.oauth_provider_name || 'SSO'}
        </Button>
      </Card.Content>
    {/if}

    {#if showPasswordForm}
      <form onsubmit={submit} class="contents">
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
          {#if config?.allow_signup ?? true}
            <p class="text-muted-foreground text-center text-sm">
              No account?
              <a href="/register" class="text-foreground underline-offset-4 hover:underline">
                Register
              </a>
            </p>
          {/if}
        </Card.Footer>
      </form>
    {/if}
  </Card.Root>
</div>
