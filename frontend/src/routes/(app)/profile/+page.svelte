<script lang="ts">
  // Profile — change display name, notifications toggle, change password
  // Wires to GET/PATCH /profile via the shared API client.
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { passkeys as passkeysApi, profile as profileApi, errorMessage } from '$lib/api';
  import type { Passkey } from '$lib/api';
  import { isCeremonyCancelled, passkeysSupported, registerPasskey } from '$lib/passkey';
  import { formatDateTime } from '$lib/format';
  import { reportUnexpected } from '$lib/report';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';
  import { cn } from '$lib/utils';
  import PageTitle from '$lib/components/page-title.svelte';

  // The root layout seeds the auth store with the current user, so we render
  // from it directly rather than refetching. `user` is always present here:
  // the (app) layout guard requires a session.
  const user = $derived(authStore.user);

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

  async function saveProfile(event: SubmitEvent) {
    event.preventDefault();
    const trimmed = displayName.trim();
    if (!trimmed) {
      toast.error('Display name cannot be empty');
      return;
    }
    savingProfile = true;
    try {
      const updated = await profileApi.update({ display_name: trimmed });
      authStore.set(updated);
      displayName = updated.display_name;
      toast.success('Profile updated');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to update profile'));
      reportUnexpected(err);
    } finally {
      savingProfile = false;
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
      toast.success(next ? 'Email notifications enabled' : 'Email notifications disabled');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to update notifications'));
      reportUnexpected(err);
    } finally {
      togglingNotifications = false;
    }
  }

  // --- Passkeys ---
  //
  // The section is rendered only when the instance has the feature enabled
  // (`/auth/config`) — the routes are `404` otherwise, so there is nothing to
  // manage. Browser support is checked separately: an unsupported browser can
  // still see and delete keys registered elsewhere, it just cannot add one.
  const passkeyEnabled = $derived($page.data.config?.passkey_enabled ?? false);
  const timezone = $derived($page.data.telemetry?.timezone ?? null);

  let passkeyList = $state<Passkey[]>([]);
  let loadingPasskeys = $state(false);
  let addingPasskey = $state(false);
  let passkeySupported = $state(false);
  let newPasskeyName = $state('');
  let busyPasskeyId = $state<string | null>(null);

  // The layout's bootstrap load has already resolved by the time this mounts,
  // so the feature flag is known here — no reactive re-fetch loop needed.
  onMount(() => {
    passkeySupported = passkeysSupported();
    if (passkeyEnabled) loadPasskeys();
  });

  async function loadPasskeys() {
    loadingPasskeys = true;
    try {
      passkeyList = await passkeysApi.list();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to load passkeys'));
      reportUnexpected(err);
    } finally {
      loadingPasskeys = false;
    }
  }

  async function addPasskey(event: SubmitEvent) {
    event.preventDefault();
    addingPasskey = true;
    try {
      const created = await registerPasskey(newPasskeyName.trim());
      passkeyList = [created, ...passkeyList];
      newPasskeyName = '';
      toast.success('Passkey added');
    } catch (err) {
      // A dismissed system dialog is a deliberate choice, not a failure.
      if (!isCeremonyCancelled(err)) {
        toast.error(errorMessage(err, 'Failed to add the passkey'));
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
      toast.success('Passkey removed');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to remove the passkey'));
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
      toast.error('Enter your current and new password');
      return;
    }
    if (newPassword !== confirmPassword) {
      toast.error('New passwords do not match');
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
      toast.success('Password changed');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to change password'));
      reportUnexpected(err);
    } finally {
      changingPassword = false;
    }
  }
</script>

<PageTitle title="Profile" />

<div class="mx-auto max-w-2xl space-y-6">
  <h1 class="text-2xl font-semibold tracking-tight">Profile</h1>

  <!-- Account details -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Account</Card.Title>
      <Card.Description>Your display name and account email.</Card.Description>
    </Card.Header>
    <form onsubmit={saveProfile} class="contents">
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <label for="email" class="text-sm font-medium">Email</label>
          <Input id="email" type="email" value={user?.email ?? ''} disabled readonly />
          <p class="text-muted-foreground text-xs">Your email address cannot be changed.</p>
        </div>
        <div class="space-y-2">
          <label for="display-name" class="text-sm font-medium">Display name</label>
          <Input
            id="display-name"
            type="text"
            autocomplete="name"
            bind:value={displayName}
            required
          />
        </div>
      </Card.Content>
      <Card.Footer class="justify-end">
        <Button type="submit" disabled={savingProfile || !displayNameDirty}>
          {savingProfile ? 'Saving…' : 'Save changes'}
        </Button>
      </Card.Footer>
    </form>
  </Card.Root>

  <!-- Notifications -->
  <Card.Root class={cn(!user?.notifications_enabled && 'ring-orange-500')}>
    <Card.Header>
      <Card.Title>Notifications</Card.Title>
      <Card.Description>Email notifications for new issues and regressions.</Card.Description>
    </Card.Header>
    <Card.Content>
      <div class="flex items-center justify-between gap-4">
        <div class="space-y-0.5">
          <p class="text-sm font-medium">Email notifications</p>
          <p class="text-muted-foreground text-sm">
            {user?.notifications_enabled
              ? 'You receive emails for new issues and regressions.'
              : 'Email notifications are turned off.'}
          </p>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={user?.notifications_enabled ?? false}
          aria-label="Toggle email notifications"
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
        <Card.Title>Passkeys</Card.Title>
        <Card.Description>
          Sign in with Touch ID, Windows Hello, or a security key instead of your password.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        {#if loadingPasskeys && passkeyList.length === 0}
          <p class="text-muted-foreground text-sm">Loading…</p>
        {:else if passkeyList.length === 0}
          <p class="text-muted-foreground text-sm">No passkeys registered yet.</p>
        {:else}
          <ul class="divide-border divide-y">
            {#each passkeyList as key (key.id)}
              <li class="flex items-center justify-between gap-4 py-3">
                <div class="min-w-0 space-y-0.5">
                  <p class="truncate text-sm font-medium">{key.name}</p>
                  <p class="text-muted-foreground text-xs">
                    Added {formatDateTime(key.created_at, timezone)}
                    {#if key.last_used_at}
                      · last used {formatDateTime(key.last_used_at, timezone)}
                    {:else}
                      · never used
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
                  {busyPasskeyId === key.id ? 'Removing…' : 'Remove'}
                </Button>
              </li>
            {/each}
          </ul>
        {/if}

        {#if passkeySupported}
          <form onsubmit={addPasskey} class="flex items-end gap-2">
            <div class="flex-1 space-y-2">
              <label for="passkey-name" class="text-sm font-medium">Name</label>
              <Input
                id="passkey-name"
                type="text"
                placeholder="MacBook Touch ID"
                bind:value={newPasskeyName}
              />
            </div>
            <Button type="submit" disabled={addingPasskey}>
              {addingPasskey ? 'Waiting…' : 'Add passkey'}
            </Button>
          </form>
        {:else}
          <p class="text-muted-foreground text-sm">
            This browser cannot create passkeys. Keys added elsewhere still work here.
          </p>
        {/if}
      </Card.Content>
    </Card.Root>
  {/if}

  <!-- Change password -->
  {#if canChangePassword}
    <Card.Root>
      <Card.Header>
        <Card.Title>Password</Card.Title>
        <Card.Description>Change the password used to sign in.</Card.Description>
      </Card.Header>
      <form onsubmit={changePassword} class="contents">
        <Card.Content class="space-y-4">
          <div class="space-y-2">
            <label for="current-password" class="text-sm font-medium">Current password</label>
            <Input
              id="current-password"
              type="password"
              autocomplete="current-password"
              bind:value={currentPassword}
              required
            />
          </div>
          <div class="space-y-2">
            <label for="new-password" class="text-sm font-medium">New password</label>
            <Input
              id="new-password"
              type="password"
              autocomplete="new-password"
              bind:value={newPassword}
              required
            />
          </div>
          <div class="space-y-2">
            <label for="confirm-password" class="text-sm font-medium">Confirm new password</label>
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
            {changingPassword ? 'Changing…' : 'Change password'}
          </Button>
        </Card.Footer>
      </form>
    </Card.Root>
  {/if}
</div>
