<script lang="ts">
  import { t } from '$lib/i18n';
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { auth, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { isCeremonyCancelled, passkeysSupported, signInWithPasskey } from '$lib/passkey';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';
  import PageTitle from '$lib/components/page-title.svelte';

  const EMAIL_STORAGE_KEY = 'soika:login:email';

  // localStorage can throw (private mode, quota, disabled storage); never let
  // remembering the email abort onMount focus logic or the submit flow.
  function readStoredEmail(): string | null {
    try {
      return localStorage.getItem(EMAIL_STORAGE_KEY);
    } catch {
      return null;
    }
  }

  function writeStoredEmail(value: string): void {
    try {
      localStorage.setItem(EMAIL_STORAGE_KEY, value);
    } catch {
      // Best-effort: ignore storage failures.
    }
  }

  // Maps callback `?error=<slug>` values to user-facing messages.
  const ERROR_MESSAGES: Record<string, string> = $derived({
    forbidden: $t('your_account_is_not_allowed_to_sign_in'),
    auth_failed: $t('sign_in_failed_please_try_again'),
    invalid_request: $t('the_sign_in_request_was_invalid_please_try_again'),
    sso_failed: $t('single_sign_on_failed_please_try_again')
  });

  let email = $state('');
  let password = $state('');
  let submitting = $state(false);
  let passkeySubmitting = $state(false);
  // Feature detection runs in the browser only (onMount), so SSR/prerender
  // never renders a button the visitor's browser cannot honour.
  let passkeySupported = $state(false);
  let emailInput = $state<HTMLInputElement | null>(null);
  let passwordInput = $state<HTMLInputElement | null>(null);

  // The root layout (`+layout.ts`) already fetches GET /auth/config once and
  // memoizes it, exposing it as `data.config` — read that instead of issuing a
  // duplicate request from this page.
  const config = $derived($page.data.config);

  const canSubmit = $derived(email.trim() !== '' && password !== '');

  // Password login stays a first-class option on the form even when SSO is on:
  // the built-in admin (and any account allowed to use a password) signs in here,
  // while SSO is offered as an additional button rather than replacing the form.
  const showSso = $derived(config?.oauth_enabled ?? false);

  // Passkeys are offered alongside the password form whenever the instance has
  // the feature on and the browser supports WebAuthn.
  const showPasskey = $derived((config?.passkey_enabled ?? false) && passkeySupported);

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
    if (slug) toast.error(ERROR_MESSAGES[slug] ?? $t('sign_in_failed'));
  });

  // The layout sets `data.config` to `null` only when the bootstrap fetch
  // failed (a healthy anonymous session still returns config). Surface that as a
  // non-blocking toast so a hard-down backend is visible instead of the form
  // silently rendering empty. The password form still works as a fallback.
  $effect(() => {
    if (config === null) toast.error($t('could_not_reach_the_server'));
  });

  onMount(() => {
    passkeySupported = passkeysSupported();

    const savedEmail = readStoredEmail();
    if (savedEmail) email = savedEmail;

    if (!email) {
      emailInput?.focus();
    } else if (!password) {
      passwordInput?.focus();
    }
  });

  // Usernameless sign-in: no email is submitted — the browser picks the
  // credential and the backend resolves the account from its user handle.
  async function signInWithPasskeyClicked() {
    passkeySubmitting = true;
    try {
      const user = await signInWithPasskey();
      authStore.set(user);
      await goto(safeNext($page.url.searchParams.get('next')) ?? '/', { invalidateAll: true });
    } catch (err) {
      // A dismissed system dialog is a deliberate choice, not a failure.
      if (!isCeremonyCancelled(err)) toast.error(errorMessage(err, $t('passkey_sign_in_failed')));
    } finally {
      passkeySubmitting = false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    try {
      const user = await auth.login({ email, password });
      writeStoredEmail(email);
      authStore.set(user);
      await goto(safeNext($page.url.searchParams.get('next')) ?? '/', { invalidateAll: true });
    } catch (err) {
      toast.error(errorMessage(err, $t('login_failed')));
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title={$t('sign_in')} />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    <Card.Header>
      <Card.Title>{$t('sign_in_to_soika')}</Card.Title>
      <Card.Description>{$t('enter_your_credentials_to_continue')}</Card.Description>
    </Card.Header>

    <form onsubmit={submit} class="contents">
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <label for="email" class="text-sm font-medium">{$t('email')}</label>
          <Input
            id="email"
            type="email"
            autocomplete="email"
            bind:value={email}
            bind:ref={emailInput}
            required
          />
        </div>
        <div class="space-y-2">
          <label for="password" class="text-sm font-medium">{$t('password')}</label>
          <Input
            id="password"
            type="password"
            autocomplete="current-password"
            bind:value={password}
            bind:ref={passwordInput}
            required
          />
        </div>
      </Card.Content>
      <Card.Footer class="flex-col items-stretch gap-3">
        <Button type="submit" disabled={submitting || !canSubmit}>
          {submitting ? $t('signing_in') : $t('sign_in')}
        </Button>
        {#if config?.allow_signup ?? true}
          <p class="text-muted-foreground text-center text-sm">
            {$t('no_account')}
            <a href="/register" class="text-foreground underline-offset-4 hover:underline">
              {$t('register')}
            </a>
          </p>
        {/if}

        {#if showPasskey}
          <div class="text-muted-foreground flex items-center gap-2 text-xs uppercase">
            <span class="bg-border h-px flex-1"></span>
            {$t('or')}
            <span class="bg-border h-px flex-1"></span>
          </div>
          <Button
            type="button"
            variant="outline"
            class="w-full"
            disabled={passkeySubmitting}
            onclick={signInWithPasskeyClicked}
          >
            {passkeySubmitting ? $t('waiting_for_your_passkey') : $t('sign_in_with_a_passkey')}
          </Button>
        {/if}

        {#if showSso}
          <!-- SSO is offered as an additional option, not a replacement for the
               password form (the built-in admin always needs the password path). -->
          <div class="text-muted-foreground flex items-center gap-2 text-xs uppercase">
            <span class="bg-border h-px flex-1"></span>
            {$t('or')}
            <span class="bg-border h-px flex-1"></span>
          </div>
          <Button variant="outline" href={ssoHref} class="w-full" data-sveltekit-reload>
            {$t('sign_in_with')}
            {config?.oauth_provider_name || 'SSO'}
          </Button>
        {/if}
      </Card.Footer>
    </form>
  </Card.Root>
</div>
