<script lang="ts">
  import { t, locale } from '$lib/i18n';
  import { invalidateAll } from '$app/navigation';
  import type { Locale } from '$lib/locale';
  // Profile — change display name, notifications toggle, change password
  // Wires to GET/PATCH /profile via the shared API client.
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { passkeys as passkeysApi, profile as profileApi, errorMessage } from '$lib/api';
  import type { Passkey } from '$lib/api';
  import { isCeremonyCancelled, passkeysSupported, registerPasskey } from '$lib/passkey';
  import { formatDateTime } from '$lib/format';
  import { MIN_DISPLAY_NAME_LENGTH, displayNameError } from '$lib/display-name';
  import { reportUnexpected } from '$lib/report';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';
  import { cn } from '$lib/utils';
  import PageTitle from '$lib/components/page-title.svelte';
  import EntityHeader from '$lib/components/entity-header.svelte';
  import StatPill from '$lib/components/stat-pill.svelte';
  import BellOff from '@lucide/svelte/icons/bell-off';

  // The root layout seeds the auth store with the current user, so we render
  // from it directly rather than refetching. `user` is always present here:
  // the (app) layout guard requires a session.
  const user = $derived(authStore.user);

  const canChangeDisplayName = $derived(
    user?.auth_provider !== 'oidc' || page.data.config?.sso_allow_display_name_change === true
  );
  // OIDC accounts have no local password, so the change-password card is hidden.
  const canChangePassword = $derived(user?.auth_provider !== 'oidc');

  // --- Account details (display name) ---
  let displayName = $state(authStore.user?.display_name ?? '');
  let savingProfile = $state(false);

  // The auth store is seeded asynchronously by the layout load, so it may be
  // `undefined` at component-init time. Re-sync the field whenever the
  // server-side value changes (initial load + after save). This only tracks
  // `user?.display_name`, so it does not clobber in-progress edits.
  $effect(() => {
    displayName = user?.display_name ?? '';
  });

  const displayNameDirty = $derived(displayName.trim() !== (user?.display_name ?? ''));
  // Only flag the field once it was edited, so a legacy short name stored before
  // the rule existed does not greet the user with an error.
  const nameError = $derived(displayNameDirty ? displayNameError(displayName, $locale) : null);

  async function saveProfile(event: SubmitEvent) {
    event.preventDefault();
    if (!canChangeDisplayName) return;
    const trimmed = displayName.trim();
    const error = displayNameError(trimmed);
    if (error) {
      toast.error(error);
      return;
    }
    savingProfile = true;
    try {
      const updated = await profileApi.update({ display_name: trimmed });
      authStore.set(updated);
      displayName = updated.display_name;
      toast.success($t('profile_updated'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_update_profile')));
      reportUnexpected(err);
    } finally {
      savingProfile = false;
    }
  }

  let savingLanguage = $state(false);
  async function changeLanguage(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const next = select.value as Locale;
    savingLanguage = true;
    try {
      const updated = await profileApi.update({ locale: next });
      authStore.set({ ...user!, ...updated });
      await invalidateAll();
      toast.success($t('language_updated'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_update_language')));
      reportUnexpected(err);
    } finally {
      savingLanguage = false;
      select.value = user?.locale ?? $locale ?? 'en';
    }
  }

  // --- Notifications toggle (auto-saves on change) ---
  let togglingNotifications = $state(false);

  async function toggleNotifications() {
    if (!user || togglingNotifications) return;
    const next = !user.notifications_enabled;
    togglingNotifications = true;
    try {
      const updated = await profileApi.update({ notifications_enabled: next });
      authStore.set(updated);
      toast.success(next ? $t('email_notifications_enabled') : $t('email_notifications_disabled'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_update_notifications')));
      reportUnexpected(err);
    } finally {
      togglingNotifications = false;
    }
  }

  // --- Passkeys ---
  //
  // The section is rendered only when the instance has the feature enabled
  // (`/api/config`) — the routes are `404` otherwise, so there is nothing to
  // manage. Browser support is checked separately: an unsupported browser can
  // still see and delete keys registered elsewhere, it just cannot add one.
  const passkeyEnabled = $derived(page.data.config?.passkey_enabled ?? false);
  const timezone = $derived(page.data.telemetry?.timezone ?? null);

  let passkeyList = $state<Passkey[]>([]);
  let addingPasskey = $state(false);
  let passkeySupported = $state(false);
  let newPasskeyName = $state('');
  let busyPasskeyId = $state<string | null>(null);

  // The layout's bootstrap load has already resolved by the time this mounts,
  // so the feature flag is known here — no reactive re-fetch loop needed.
  onMount(() => {
    passkeySupported = passkeysSupported();
    if (passkeyEnabled) passkeyList = page.data.config?.passkeys ?? [];
  });

  async function addPasskey(event: SubmitEvent) {
    event.preventDefault();
    addingPasskey = true;
    try {
      const created = await registerPasskey(newPasskeyName.trim());
      passkeyList = [created, ...passkeyList];
      newPasskeyName = '';
      await invalidateAll();
      toast.success($t('passkey_added'));
    } catch (err) {
      // A dismissed system dialog is a deliberate choice, not a failure.
      if (!isCeremonyCancelled(err)) {
        toast.error(errorMessage(err, $t('failed_to_add_the_passkey')));
        reportUnexpected(err);
      }
    } finally {
      addingPasskey = false;
    }
  }

  async function removePasskey(key: Passkey) {
    busyPasskeyId = key.id;
    try {
      await passkeysApi.remove(key.id);
      passkeyList = passkeyList.filter((k) => k.id !== key.id);
      await invalidateAll();
      toast.success($t('passkey_removed'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_remove_the_passkey')));
      reportUnexpected(err);
    } finally {
      busyPasskeyId = null;
    }
  }

  // --- Change password ---
  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let changingPassword = $state(false);

  const canChangePasswordSubmit = $derived(
    currentPassword.length > 0 &&
      newPassword.length > 0 &&
      confirmPassword.length > 0 &&
      newPassword === confirmPassword
  );

  async function changePassword(event: SubmitEvent) {
    event.preventDefault();
    if (!currentPassword || !newPassword) {
      toast.error($t('enter_your_current_and_new_password'));
      return;
    }
    if (newPassword !== confirmPassword) {
      toast.error($t('new_passwords_do_not_match'));
      return;
    }
    changingPassword = true;
    try {
      const updated = await profileApi.update({
        current_password: currentPassword,
        new_password: newPassword
      });
      authStore.set(updated);
      currentPassword = '';
      newPassword = '';
      confirmPassword = '';
      toast.success($t('password_changed'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_change_password')));
      reportUnexpected(err);
    } finally {
      changingPassword = false;
    }
  }
</script>

<PageTitle title={$t('profile')} />

<div class="max-w-2xl space-y-6">
  <EntityHeader title={user?.display_name ?? 'Profile'} avatarId={user?.id} avatarShape="circle">
    {#snippet pills()}
      {#if user}
        <StatPill>{user.email}</StatPill>
        <StatPill><strong class="capitalize">{user.instance_role}</strong></StatPill>
        {#if user.auth_provider === 'oidc'}
          <StatPill>{$t('single_sign_on')}</StatPill>
        {/if}
        {#if !user.notifications_enabled}
          <StatPill class="text-orange-600 dark:text-orange-400">
            <BellOff />
            {$t('notifications_off')}
          </StatPill>
        {/if}
      {/if}
    {/snippet}
  </EntityHeader>

  <!-- Account details -->
  <Card.Root>
    <Card.Header>
      <Card.Title>{$t('account')}</Card.Title>
      <Card.Description>{$t('your_display_name_and_account_email')}</Card.Description>
    </Card.Header>
    <form onsubmit={saveProfile} class="contents">
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <label for="email" class="text-sm font-medium">{$t('email')}</label>
          <Input id="email" type="email" value={user?.email ?? ''} disabled readonly />
          <p class="text-muted-foreground text-xs">{$t('your_email_address_cannot_be_changed')}</p>
        </div>
        <div class="space-y-2">
          <label for="display-name" class="text-sm font-medium">{$t('display_name')}</label>
          <Input
            id="display-name"
            type="text"
            autocomplete="name"
            bind:value={displayName}
            disabled={!canChangeDisplayName}
            required
            minlength={MIN_DISPLAY_NAME_LENGTH}
            aria-invalid={nameError !== null}
            aria-describedby="display-name-hint"
          />
          <p
            id="display-name-hint"
            class={cn('text-xs', nameError ? 'text-destructive' : 'text-muted-foreground')}
          >
            {#if canChangeDisplayName}
              {nameError ??
                $t('minimum_characters', { values: { count: MIN_DISPLAY_NAME_LENGTH } })}
            {:else}
              {$t('sso_display_name_change_disabled')}
            {/if}
          </p>
        </div>
      </Card.Content>
      <Card.Footer class="justify-end">
        <Button
          type="submit"
          disabled={!canChangeDisplayName ||
            savingProfile ||
            !displayNameDirty ||
            nameError !== null}
        >
          {savingProfile ? $t('saving') : $t('save_changes')}
        </Button>
      </Card.Footer>
    </form>
  </Card.Root>

  <Card.Root>
    <Card.Header>
      <Card.Title>{$t('language')}</Card.Title>
      <Card.Description>{$t('your_language_preference_is_saved_to_your_account')}</Card.Description>
    </Card.Header>
    <Card.Content>
      <label for="language" class="mb-2 block text-sm font-medium">{$t('interface_language')}</label
      >
      <select
        id="language"
        class="border-input bg-background h-9 w-full rounded-md border px-3 text-sm"
        value={$locale ?? 'en'}
        disabled={savingLanguage}
        onchange={changeLanguage}
      >
        <option value="en">English</option>
        <option value="ru">Русский</option>
      </select>
    </Card.Content>
  </Card.Root>

  <!-- Notifications -->
  <Card.Root class={cn(!user?.notifications_enabled && 'ring-orange-500')}>
    <Card.Header>
      <Card.Title>{$t('notifications')}</Card.Title>
      <Card.Description>{$t('email_notifications_for_new_issues_and_regressions')}</Card.Description
      >
    </Card.Header>
    <Card.Content>
      <div class="flex items-center justify-between gap-4">
        <div class="space-y-0.5">
          <p class="text-sm font-medium">{$t('email_notifications')}</p>
          <p class="text-muted-foreground text-sm">
            {user?.notifications_enabled
              ? $t('you_receive_emails_for_new_issues_and_regressions')
              : $t('email_notifications_are_turned_off')}
          </p>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={user?.notifications_enabled ?? false}
          aria-label={$t('toggle_email_notifications')}
          disabled={togglingNotifications}
          onclick={toggleNotifications}
          class={cn(
            'relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full transition-colors',
            'focus-visible:ring-primary/30 focus-visible:ring-offset-background focus-visible:ring-1 focus-visible:ring-offset-2 focus-visible:outline-none',
            'disabled:cursor-not-allowed disabled:opacity-50',
            user?.notifications_enabled ? 'bg-primary' : 'bg-input'
          )}
        >
          <span
            class={cn(
              'bg-background pointer-events-none inline-block size-4 transform rounded-full shadow-sm ring-0 transition-transform',
              user?.notifications_enabled ? 'translate-x-4' : 'translate-x-0.5'
            )}
          ></span>
        </button>
      </div>
    </Card.Content>
  </Card.Root>

  <!-- Passkeys -->
  {#if passkeyEnabled}
    <Card.Root>
      <Card.Header>
        <Card.Title>{$t('passkeys')}</Card.Title>
        <Card.Description>
          {$t('sign_in_with_touch_id_windows_hello_or_a_security_key_instead_of_your_password')}
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        {#if passkeyList.length === 0}
          <p class="text-muted-foreground text-sm">{$t('no_passkeys_registered_yet')}</p>
        {:else}
          <ul class="divide-border divide-y">
            {#each passkeyList as key (key.id)}
              <li class="flex items-center justify-between gap-4 py-3">
                <div class="min-w-0 space-y-0.5">
                  <p class="truncate text-sm font-medium">{key.name}</p>
                  <p class="text-muted-foreground text-xs">
                    {$t('added')}
                    {formatDateTime(key.created_at, timezone, $locale)}
                    {#if key.last_used_at}
                      {$t('last_used')} {formatDateTime(key.last_used_at, timezone, $locale)}
                    {:else}
                      {$t('never_used')}
                    {/if}
                  </p>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={busyPasskeyId === key.id}
                  onclick={() => removePasskey(key)}
                >
                  {busyPasskeyId === key.id ? $t('removing') : $t('remove')}
                </Button>
              </li>
            {/each}
          </ul>
        {/if}

        {#if passkeySupported}
          <form onsubmit={addPasskey} class="flex items-end gap-2">
            <div class="flex-1 space-y-2">
              <label for="passkey-name" class="text-sm font-medium">{$t('name')}</label>
              <Input
                id="passkey-name"
                type="text"
                placeholder="MacBook Touch ID"
                bind:value={newPasskeyName}
              />
            </div>
            <Button type="submit" disabled={addingPasskey}>
              {addingPasskey ? $t('waiting') : $t('add_passkey')}
            </Button>
          </form>
        {:else}
          <p class="text-muted-foreground text-sm">
            {$t('this_browser_cannot_create_passkeys_keys_added_elsewhere_still_work_here')}
          </p>
        {/if}
      </Card.Content>
    </Card.Root>
  {/if}

  <!-- Change password -->
  {#if canChangePassword}
    <Card.Root>
      <Card.Header>
        <Card.Title>{$t('password')}</Card.Title>
        <Card.Description>{$t('change_the_password_used_to_sign_in')}</Card.Description>
      </Card.Header>
      <form onsubmit={changePassword} class="contents">
        <Card.Content class="space-y-4">
          <div class="space-y-2">
            <label for="current-password" class="text-sm font-medium"
              >{$t('current_password')}</label
            >
            <Input
              id="current-password"
              type="password"
              autocomplete="current-password"
              bind:value={currentPassword}
              required
            />
          </div>
          <div class="space-y-2">
            <label for="new-password" class="text-sm font-medium">{$t('new_password')}</label>
            <Input
              id="new-password"
              type="password"
              autocomplete="new-password"
              bind:value={newPassword}
              required
            />
          </div>
          <div class="space-y-2">
            <label for="confirm-password" class="text-sm font-medium"
              >{$t('confirm_new_password')}</label
            >
            <Input
              id="confirm-password"
              type="password"
              autocomplete="new-password"
              bind:value={confirmPassword}
              required
            />
          </div>
        </Card.Content>
        <Card.Footer class="justify-end">
          <Button type="submit" disabled={changingPassword || !canChangePasswordSubmit}>
            {changingPassword ? $t('changing') : $t('change_password')}
          </Button>
        </Card.Footer>
      </form>
    </Card.Root>
  {/if}
</div>
