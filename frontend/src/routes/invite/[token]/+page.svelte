<script lang="ts">
  import { t } from '$lib/i18n';
  import { MIN_DISPLAY_NAME_LENGTH } from '$lib/display-name';
  import { untrack } from 'svelte';
  import { goto } from '$app/navigation';
  import { invites, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { Badge } from '$lib/components/ui/badge';
  import { toast } from '$lib/components/ui/sonner';
  import PageTitle from '$lib/components/page-title.svelte';
  import { teamRoleDescription } from '$lib/components/role-hint.svelte';
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
      toast.success($t('invite_accepted'));
      await goto(`/teams/${data.preview.team_id}`, { invalidateAll: true });
    } catch (err) {
      toast.error(errorMessage(err, $t('could_not_accept_invite')));
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title={$t('invite')} />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    {#if data.error || !data.preview}
      <Card.Header>
        <Card.Title>{$t('invite_unavailable')}</Card.Title>
        <Card.Description>{data.error ?? $t('this_invite_is_no_longer_valid')}</Card.Description>
      </Card.Header>
      <Card.Footer>
        <Button href="/login" variant="outline">{$t('go_to_sign_in')}</Button>
      </Card.Footer>
    {:else}
      <Card.Header>
        <Card.Title>{$t('you_re_invited')}</Card.Title>
        <Card.Description>
          {$t('join')} <span class="font-medium">{data.preview.team_name}</span>
          {$t('as')}
          <Badge variant="secondary" class="capitalize">{$t(data.preview.role)}</Badge>
        </Card.Description>
        <p class="text-muted-foreground text-xs">{teamRoleDescription(data.preview.role)}</p>
      </Card.Header>
      <form onsubmit={accept} class="contents">
        <Card.Content class="space-y-4">
          <div class="space-y-2">
            <label for="email" class="text-sm font-medium">{$t('email')}</label>
            <Input id="email" type="email" bind:value={email} required />
          </div>
          {#if requiresRegistration}
            <div class="space-y-2">
              <label for="name" class="text-sm font-medium">{$t('display_name')}</label>
              <Input
                id="name"
                bind:value={displayName}
                required
                minlength={MIN_DISPLAY_NAME_LENGTH}
              />
            </div>
          {/if}
          <div class="space-y-2">
            <label for="password" class="text-sm font-medium">{$t('password')}</label>
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
            {submitting ? $t('joining') : $t('accept_invite')}
          </Button>
        </Card.Footer>
      </form>
    {/if}
  </Card.Root>
</div>
