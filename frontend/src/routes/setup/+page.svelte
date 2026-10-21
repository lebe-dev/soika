<script lang="ts">
  import { t } from '$lib/i18n';
  import { MIN_DISPLAY_NAME_LENGTH } from '$lib/display-name';
  import { goto } from '$app/navigation';
  import { auth, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';
  import PageTitle from '$lib/components/page-title.svelte';

  // First-run setup: creates the built-in instance admin and names the
  // organization. The root layout only routes here while the instance is
  // uninitialized; once an admin exists it redirects to /login, and the
  // backend `POST /auth/setup` rejects a second call regardless.
  let orgName = $state('');
  let displayName = $state('');
  let email = $state('');
  let password = $state('');
  let confirmPassword = $state('');
  let submitting = $state(false);

  const passwordsMatch = $derived(password === confirmPassword);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!passwordsMatch) {
      toast.error($t('passwords_do_not_match_d69c3b'));
      return;
    }
    submitting = true;
    try {
      const user = await auth.setup({
        email,
        password,
        display_name: displayName,
        org_name: orgName
      });
      authStore.set(user);
      toast.success($t('admin_account_created'));
      await goto('/', { invalidateAll: true });
    } catch (err) {
      toast.error(errorMessage(err, $t('setup_failed')));
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title={$t('setup')} />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    <Card.Header>
      <Card.Title>{$t('welcome_to_soika')}</Card.Title>
      <Card.Description>
        {$t('set_up_your_instance_by_creating_the_administrator_account')}
      </Card.Description>
    </Card.Header>
    <form onsubmit={submit} class="contents">
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <label for="org" class="text-sm font-medium">{$t('organization_name')}</label>
          <Input id="org" autocomplete="organization" bind:value={orgName} required />
        </div>
        <div class="space-y-2">
          <label for="name" class="text-sm font-medium">{$t('your_name')}</label>
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
        <div class="space-y-2">
          <label for="confirm" class="text-sm font-medium">{$t('confirm_password')}</label>
          <Input
            id="confirm"
            type="password"
            autocomplete="new-password"
            bind:value={confirmPassword}
            minlength={8}
            required
          />
          {#if confirmPassword && !passwordsMatch}
            <p class="text-destructive text-sm">{$t('passwords_do_not_match')}</p>
          {/if}
        </div>
      </Card.Content>
      <Card.Footer>
        <Button type="submit" class="w-full" disabled={submitting || !passwordsMatch}>
          {submitting ? $t('creating') : $t('create_admin_account')}
        </Button>
      </Card.Footer>
    </form>
  </Card.Root>
</div>
