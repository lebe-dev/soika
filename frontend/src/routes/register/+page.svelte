<script lang="ts">
  import { t } from '$lib/i18n';
  import { MIN_DISPLAY_NAME_LENGTH } from '$lib/display-name';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { auth, ApiError, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';
  import PageTitle from '$lib/components/page-title.svelte';

  // Register — only succeeds when the instance has `allow_signup` enabled
  // and SSO is off. The bootstrap `GET /api/config` is fetched once by the root
  // layout (`data.config`); we read it here to disable the form when signup is
  // closed or OAuth replaces local accounts. A 403 from the register endpoint is
  // also handled as a fallback.
  let email = $state('');
  let displayName = $state('');
  let password = $state('');
  let submitting = $state(false);
  let signupDisabled = $state(false);
  const config = $derived(page.data.config);

  // `null` config => the layout bootstrap failed; keep the form available until we
  // know (the register endpoint still gates). Surface a non-blocking toast so the
  // anonymous-route failure isn't silent.
  $effect(() => {
    if (config === null) {
      toast.error($t('could_not_reach_the_server'));
    }
  });

  const registrationClosed = $derived(
    signupDisabled || (config != null && (!config.allow_signup || config.oauth_enabled))
  );

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    try {
      const user = await auth.register({ email, password, display_name: displayName });
      authStore.set(user);
      toast.success($t('account_created'));
      await goto('/', { invalidateAll: true });
    } catch (err) {
      if (err instanceof ApiError && err.isForbidden) {
        signupDisabled = true;
        return;
      }
      toast.error(errorMessage(err, $t('registration_failed')));
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title={$t('register')} />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    {#if registrationClosed}
      <Card.Header>
        <Card.Title>{$t('sign_up_is_disabled')}</Card.Title>
        <Card.Description>
          {$t('public_registration_is_turned_off_for_this_instance_ask_an_administrator_for_an_')}
        </Card.Description>
      </Card.Header>
      <Card.Footer>
        <Button href="/login" variant="outline" class="w-full">{$t('go_to_sign_in')}</Button>
      </Card.Footer>
    {:else}
      <Card.Header>
        <Card.Title>{$t('create_your_account')}</Card.Title>
        <Card.Description>{$t('registration_must_be_enabled_by_an_administrator')}</Card.Description
        >
      </Card.Header>
      <form onsubmit={submit} class="contents">
        <Card.Content class="space-y-4">
          <div class="space-y-2">
            <label for="name" class="text-sm font-medium">{$t('display_name')}</label>
            <Input
              id="name"
              autocomplete="name"
              bind:value={displayName}
              required
              minlength={MIN_DISPLAY_NAME_LENGTH}
            />
          </div>
          <div class="space-y-2">
            <label for="email" class="text-sm font-medium">{$t('email')}</label>
            <Input id="email" type="email" autocomplete="email" bind:value={email} required />
          </div>
          <div class="space-y-2">
            <label for="password" class="text-sm font-medium">{$t('password')}</label>
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
            {submitting ? $t('creating') : $t('create_account')}
          </Button>
          <p class="text-muted-foreground text-center text-sm">
            {$t('already_have_an_account')}
            <a href="/login" class="text-foreground underline-offset-4 hover:underline"
              >{$t('sign_in')}</a
            >
          </p>
        </Card.Footer>
      </form>
    {/if}
  </Card.Root>
</div>
