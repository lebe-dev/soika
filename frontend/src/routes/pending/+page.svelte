<script lang="ts">
  import { t } from '$lib/i18n';
  import { page } from '$app/stores';
  import { Button } from '$lib/components/ui/button';
  import * as Card from '$lib/components/ui/card';
  import Clock from '@lucide/svelte/icons/clock';
  import { toast } from '$lib/components/ui/sonner';
  import PageTitle from '$lib/components/page-title.svelte';

  // Awaiting-approval screen. The OIDC callback redirects here when an account
  // is provisioned under OAUTH_REQUIRE_APPROVAL: it exists but holds no session
  // until an instance admin approves it. There is nothing for the visitor to do
  // but wait and retry sign-in later.
  //
  // The bootstrap `GET /api/config` is fetched once by the root layout
  // (`data.config`); we read it here for the provider name. The page is
  // informational and renders fine without it.
  const config = $derived($page.data.config);

  // Surface a non-blocking toast if the layout bootstrap failed, so the
  // anonymous-route failure isn't silent.
  $effect(() => {
    if (config === null) {
      toast.error($t('could_not_reach_the_server'));
    }
  });
</script>

<PageTitle title={$t('awaiting_approval')} />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    <Card.Header>
      <div
        class="bg-muted text-muted-foreground mb-2 flex size-10 items-center justify-center rounded-full"
      >
        <Clock class="size-5" />
      </div>
      <Card.Title>{$t('account_awaiting_approval')}</Card.Title>
      <Card.Description>
        {$t('you_ve_signed_in_successfully_but_your_account_needs_to_be_approved_by_an_admini')}
      </Card.Description>
    </Card.Header>
    <Card.Content class="space-y-4">
      <p class="text-muted-foreground text-sm">
        {$t('once_an_administrator_approves_your_account_sign_in_again_to_access')}
        {config?.oauth_provider_name ? `${config.oauth_provider_name}` : $t('the_app')}.
      </p>
      <Button href="/login" class="w-full" data-sveltekit-reload>{$t('back_to_sign_in')}</Button>
    </Card.Content>
  </Card.Root>
</div>
