<script lang="ts">
  import { auth, type AuthConfig } from '$lib/api';
  import { Button } from '$lib/components/ui/button';
  import * as Card from '$lib/components/ui/card';
  import Clock from '@lucide/svelte/icons/clock';
  import PageTitle from '$lib/components/page-title.svelte';

  // Awaiting-approval screen. The OIDC callback redirects here when an account
  // is provisioned under OAUTH_REQUIRE_APPROVAL: it exists but holds no session
  // until an instance admin approves it. There is nothing for the visitor to do
  // but wait and retry sign-in later.
  let config = $state<AuthConfig | null>(null);

  $effect(() => {
    void loadConfig();
  });

  async function loadConfig() {
    try {
      config = await auth.config();
    } catch {
      // Best-effort: the page is informational and renders fine without config.
    }
  }
</script>

<PageTitle title="Awaiting approval" />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    <Card.Header>
      <div
        class="bg-muted text-muted-foreground mb-2 flex size-10 items-center justify-center rounded-full"
      >
        <Clock class="size-5" />
      </div>
      <Card.Title>Account awaiting approval</Card.Title>
      <Card.Description>
        You've signed in successfully, but your account needs to be approved by an administrator
        before you can continue.
      </Card.Description>
    </Card.Header>
    <Card.Content class="space-y-4">
      <p class="text-muted-foreground text-sm">
        Once an administrator approves your account, sign in again to access {config?.oauth_provider_name
          ? `${config.oauth_provider_name}`
          : 'the app'}.
      </p>
      <Button href="/login" class="w-full" data-sveltekit-reload>Back to sign in</Button>
    </Card.Content>
  </Card.Root>
</div>
