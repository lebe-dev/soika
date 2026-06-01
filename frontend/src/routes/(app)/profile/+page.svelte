<script lang="ts">
  // Profile — change display name, notifications toggle, change password
  // (MVP §10.3, §15). Wires to GET/PATCH /profile via the shared API client.
  import { profile as profileApi, errorMessage } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import { toast } from '$lib/components/ui/sonner';
  import { cn } from '$lib/utils';

  // The root layout seeds the auth store with the current user, so we render
  // from it directly rather than refetching. `user` is always present here:
  // the (app) layout guard requires a session.
  const user = $derived(authStore.user);

  // --- Account details (display name) ---
  let displayName = $state(authStore.user?.display_name ?? '');
  let savingProfile = $state(false);

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
    } finally {
      togglingNotifications = false;
    }
  }

  // --- Change password ---
  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let changingPassword = $state(false);

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
    } finally {
      changingPassword = false;
    }
  }
</script>

<div class="mx-auto max-w-2xl space-y-6">
  <h1 class="text-2xl font-semibold tracking-tight">Profile</h1>

  <!-- Account details -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Account</Card.Title>
      <Card.Description>Your display name and account email.</Card.Description>
    </Card.Header>
    <form onsubmit={saveProfile}>
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
  <Card.Root>
    <Card.Header>
      <Card.Title>Notifications</Card.Title>
      <Card.Description>
        Email notifications for new issues and regressions (MVP §12).
      </Card.Description>
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
            'relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full transition-colors',
            'focus-visible:ring-ring focus-visible:ring-offset-background focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none',
            'disabled:cursor-not-allowed disabled:opacity-50',
            user?.notifications_enabled ? 'bg-primary' : 'bg-input'
          )}
        >
          <span
            class={cn(
              'bg-background pointer-events-none inline-block size-5 transform rounded-full shadow-sm ring-0 transition-transform',
              user?.notifications_enabled ? 'translate-x-5' : 'translate-x-0.5'
            )}
          ></span>
        </button>
      </div>
    </Card.Content>
  </Card.Root>

  <!-- Change password -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Password</Card.Title>
      <Card.Description>Change the password used to sign in.</Card.Description>
    </Card.Header>
    <form onsubmit={changePassword}>
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
        <Button type="submit" disabled={changingPassword}>
          {changingPassword ? 'Changing…' : 'Change password'}
        </Button>
      </Card.Footer>
    </form>
  </Card.Root>
</div>
