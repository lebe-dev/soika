<script lang="ts">
  import { untrack } from 'svelte';
  import { version } from '$app/environment';
  import { invalidateAll } from '$app/navigation';
  import { api, errorMessage, type InstanceRole, type UpdateSettingsRequest } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import { formatDate } from '$lib/format';
  import { authStore } from '$lib/stores/auth.svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Tabs from '$lib/components/ui/tabs';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { NativeSelect } from '$lib/components/ui/native-select';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { toast } from '$lib/components/ui/sonner';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Check from '@lucide/svelte/icons/check';
  import Clock from '@lucide/svelte/icons/clock';
  import UserCheck from '@lucide/svelte/icons/user-check';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Mail from '@lucide/svelte/icons/mail';
  import MailX from '@lucide/svelte/icons/mail-x';
  import PageTitle from '$lib/components/page-title.svelte';
  import RoleHint from '$lib/components/role-hint.svelte';
  import PageHeader from '$lib/components/page-header.svelte';
  import Avatar from '$lib/components/avatar.svelte';
  import TeamList from '$lib/components/team-list.svelte';
  import Info from '@lucide/svelte/icons/info';
  import type { PageData } from './$types';

  // Admin — service settings, users, and teams overview.
  // Visible to instance managers (Owner | Manager); guarded in +page.ts.
  let { data }: { data: PageData } = $props();

  // Whether the current user is the instance Owner. Only an Owner may grant or
  // revoke the Owner role (the backend enforces this; the UI mirrors it).
  const isOwner = $derived(authStore.isOwner);

  // Number of Owners on the instance — used for last-Owner protection: the only
  // remaining Owner cannot be demoted (which would lock everyone out of
  // Owner-only operations).
  const ownerCount = $derived(data.users.filter((u) => u.instance_role === 'owner').length);

  // Editable service-settings form state, seeded once from the loaded settings.
  // These are mutated locally before save, so they stay `$state` (not a derived
  // mirror); `untrack` makes the seed an explicit one-time read of `data` (not a
  // reactive dependency). After a successful save we `invalidateAll()` and
  // `reset()` re-syncs the form to the freshly persisted `data.settings` values.
  let orgName = $state(untrack(() => data.settings.org_name));
  let allowSignup = $state(untrack(() => data.settings.allow_signup));
  let saving = $state(false);

  const smtp = $derived(data.settings.smtp);

  // Test-email state: an optional recipient (blank → the current admin) and an
  // in-flight flag so the button can show progress and avoid double-sends.
  let testRecipient = $state('');
  let sendingTest = $state(false);

  async function sendTestEmail() {
    sendingTest = true;
    try {
      const to = testRecipient.trim();
      const result = await api.settings.testEmail(to ? { to } : {});
      toast.success(`Test email sent to ${result.sent_to}`);
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to send test email'));
      reportUnexpected(err);
    } finally {
      sendingTest = false;
    }
  }

  // Dirty only when an editable field differs from the persisted value.
  const dirty = $derived(
    orgName.trim() !== data.settings.org_name || allowSignup !== data.settings.allow_signup
  );

  async function save() {
    const name = orgName.trim();
    if (!name) {
      toast.error('Organization name cannot be empty');
      return;
    }

    const body: UpdateSettingsRequest = {};
    if (name !== data.settings.org_name) body.org_name = name;
    if (allowSignup !== data.settings.allow_signup) body.allow_signup = allowSignup;

    saving = true;
    try {
      await api.settings.update(body);
      toast.success('Service settings saved');
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to save settings'));
      reportUnexpected(err);
    } finally {
      saving = false;
    }
  }

  function reset() {
    orgName = data.settings.org_name;
    allowSignup = data.settings.allow_signup;
  }

  // Per-user in-flight flag so approve/reject buttons disable while the request
  // runs and we avoid double-submits.
  let busyUserId = $state<string | null>(null);

  // Count of accounts awaiting approval — surfaced on the Users tab label.
  const pendingCount = $derived(data.users.filter((u) => u.status === 'pending').length);

  async function approveUser(id: string) {
    busyUserId = id;
    try {
      await api.admin.approve(id);
      toast.success('Account approved');
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to approve account'));
      reportUnexpected(err);
    } finally {
      busyUserId = null;
    }
  }

  // Whether the current user may change `role` for the user `u`. An Owner may
  // change anyone; a Manager may not touch an Owner nor grant Owner. The last
  // remaining Owner cannot be demoted away from Owner.
  function canEditRole(u: { id: string; instance_role: InstanceRole }): boolean {
    if (!isOwner && u.instance_role === 'owner') return false;
    return true;
  }

  // Whether `target` may be assigned `role` given the current actor and the
  // last-Owner rule. Disables individual <option>s in the selector.
  function roleOptionDisabled(u: { instance_role: InstanceRole }, role: InstanceRole): boolean {
    // Only an Owner can grant the Owner role.
    if (role === 'owner' && !isOwner) return true;
    // The last Owner cannot be demoted away from Owner.
    if (u.instance_role === 'owner' && role !== 'owner' && ownerCount <= 1) return true;
    return false;
  }

  async function changeRole(id: string, role: InstanceRole) {
    busyUserId = id;
    try {
      await api.admin.setRole(id, { instance_role: role });
      toast.success('Role updated');
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to update role'));
      reportUnexpected(err);
      // Re-run the load so the (optimistically-changed) <select> snaps back to
      // the server's authoritative value on failure.
      await invalidateAll();
    } finally {
      busyUserId = null;
    }
  }

  async function rejectUser(id: string, email: string) {
    if (!confirm(`Reject and delete the pending account ${email}? This cannot be undone.`)) return;
    busyUserId = id;
    try {
      await api.admin.remove(id);
      toast.success('Account rejected');
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to reject account'));
      reportUnexpected(err);
    } finally {
      busyUserId = null;
    }
  }

  // Whether the current manager may delete `u`. Deleting an Owner is Owner-only
  // and the last Owner can never be deleted (matches the backend rules). Never
  // offer deletion of one's own account.
  function canDelete(u: { id: string; instance_role: InstanceRole }, isSelf: boolean): boolean {
    if (isSelf) return false;
    if (u.instance_role === 'owner') return isOwner && ownerCount > 1;
    return true;
  }

  async function deleteUser(id: string, email: string) {
    if (
      !confirm(
        `Delete the account ${email}? This permanently removes the user and cannot be undone.`
      )
    )
      return;
    busyUserId = id;
    try {
      await api.admin.remove(id);
      toast.success('Account deleted');
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to delete account'));
      reportUnexpected(err);
    } finally {
      busyUserId = null;
    }
  }
</script>

<PageTitle title="Admin" />

<div class="space-y-6">
  <PageHeader title="Admin">
    {#snippet description()}
      Instance-wide settings, users, and teams.
    {/snippet}
  </PageHeader>

  <Tabs.Root value="settings">
    <Tabs.List>
      <Tabs.Trigger value="settings">Service settings</Tabs.Trigger>
      <span aria-hidden="true" class="bg-border h-4 w-px self-center"></span>
      <Tabs.Trigger value="users">
        Users ({data.users.length})
        {#if pendingCount > 0}
          <Badge variant="default" class="ml-1.5">{pendingCount} pending</Badge>
        {/if}
      </Tabs.Trigger>
      <span aria-hidden="true" class="bg-border h-4 w-px self-center"></span>
      <Tabs.Trigger value="teams">Teams ({data.teams.length})</Tabs.Trigger>
    </Tabs.List>

    <!-- Service settings -->
    <Tabs.Content value="settings" class="space-y-6">
      <Card.Root>
        <Card.Header>
          <Card.Title>Organization</Card.Title>
          <Card.Description>The display name shown across the instance.</Card.Description>
        </Card.Header>
        <Card.Content class="space-y-4">
          <div class="grid gap-2">
            <label for="org-name" class="text-sm font-medium">Organization name</label>
            <Input id="org-name" bind:value={orgName} class="max-w-sm" disabled={saving} />
          </div>
        </Card.Content>
      </Card.Root>

      <Card.Root>
        <Card.Header>
          <Card.Title>Registration</Card.Title>
          <Card.Description>
            Control whether new users can register without an invite.
          </Card.Description>
        </Card.Header>
        <Card.Content>
          <div class="flex items-center justify-between gap-4 rounded-lg border p-4">
            <div class="space-y-0.5">
              <p class="text-sm font-medium">Allow public sign-up</p>
              <p class="text-muted-foreground text-sm">
                When off, accounts are created only via invite.
              </p>
            </div>
            <Button
              variant={allowSignup ? 'default' : 'outline'}
              size="sm"
              disabled={saving}
              aria-pressed={allowSignup}
              onclick={() => (allowSignup = !allowSignup)}
            >
              {#if allowSignup}
                <Check class="size-4" />
                Enabled
              {:else}
                Disabled
              {/if}
            </Button>
          </div>
        </Card.Content>
      </Card.Root>

      <Card.Root>
        <Card.Header>
          <Card.Title>SMTP</Card.Title>
          <Card.Description>
            Email delivery is configured via environment variables and shown here read-only.
          </Card.Description>
        </Card.Header>
        <Card.Content class="space-y-3">
          <div class="flex items-center gap-2">
            {#if smtp.configured}
              <Badge variant="secondary">
                <Mail class="size-3" />
                Configured
              </Badge>
            {:else}
              <Badge variant="outline">
                <MailX class="size-3" />
                Not configured
              </Badge>
            {/if}
          </div>
          {#if smtp.configured}
            <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
              <dt class="text-muted-foreground">Host</dt>
              <dd class="font-mono">{smtp.host ?? '—'}</dd>
              <dt class="text-muted-foreground">Port</dt>
              <dd class="font-mono">{smtp.port ?? '—'}</dd>
              <dt class="text-muted-foreground">From</dt>
              <dd class="font-mono">{smtp.from ?? '—'}</dd>
            </dl>
            <div class="space-y-2 border-t pt-3">
              <p class="text-sm font-medium">Send a test email</p>
              <p class="text-muted-foreground text-sm">
                Verify delivery end-to-end. Leave the field blank to send to your own address.
              </p>
              <div class="flex flex-wrap items-center gap-2">
                <Input
                  type="email"
                  placeholder="recipient@example.com (optional)"
                  bind:value={testRecipient}
                  class="max-w-xs"
                  disabled={sendingTest}
                />
                <Button variant="outline" size="sm" onclick={sendTestEmail} disabled={sendingTest}>
                  <Mail class="size-4" />
                  {sendingTest ? 'Sending…' : 'Send test email'}
                </Button>
              </div>
            </div>
          {:else}
            <p class="text-muted-foreground text-sm">
              Set <code class="font-mono">SMTP_HOST</code> and related variables to enable email notifications
              and invite delivery. Invite links remain copyable without it.
            </p>
          {/if}
        </Card.Content>
      </Card.Root>

      <div class="flex items-center gap-2">
        <Button onclick={save} disabled={!dirty || saving}>
          {saving ? 'Saving…' : 'Save changes'}
        </Button>
        <Button variant="ghost" onclick={reset} disabled={!dirty || saving}>Reset</Button>
      </div>
    </Tabs.Content>

    <!-- Users overview: same roster layout as team members. -->
    <Tabs.Content value="users">
      <Card.Root class="shadow-sm">
        <Card.Header>
          <div class="flex items-center gap-1.5">
            <Card.Title>Users</Card.Title>
            <DropdownMenu.Root>
              <DropdownMenu.Trigger>
                <button
                  type="button"
                  class="text-muted-foreground hover:text-foreground transition-colors"
                  aria-label="What can each role do?"
                >
                  <Info class="size-4" />
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Content align="start" class="w-80 p-3">
                <RoleHint scope="instance" />
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </div>
          <Card.Description>All accounts on this instance.</Card.Description>
        </Card.Header>
        <Card.Content>
          {#if data.users.length === 0}
            <p class="text-muted-foreground text-sm">No users yet.</p>
          {:else}
            <div class="divide-border/60 divide-y">
              {#each data.users as u (u.id)}
                {@const isSelf = authStore.user != null && u.id === authStore.user.id}
                <div class="flex flex-wrap items-center gap-3 py-3 first:pt-0 last:pb-0">
                  <Avatar name={u.display_name} id={u.id} />
                  <div class="min-w-0 flex-1">
                    <p class="flex items-center gap-2 font-medium">
                      <span class="truncate">{u.display_name}</span>
                      {#if isSelf}
                        <span class="text-muted-foreground text-xs font-normal">(you)</span>
                      {/if}
                      {#if u.status === 'pending'}
                        <Badge variant="secondary">
                          <Clock class="size-3" />
                          Pending
                        </Badge>
                      {/if}
                    </p>
                    <p class="text-muted-foreground truncate text-sm">
                      {u.email} · joined {formatDate(u.created_at)}
                      {#if !u.notifications_enabled}
                        · notifications off
                      {/if}
                    </p>
                  </div>
                  {#if canEditRole(u)}
                    <NativeSelect
                      aria-label={`Instance role for ${u.display_name}`}
                      value={u.instance_role}
                      disabled={busyUserId === u.id}
                      onchange={(e) => changeRole(u.id, e.currentTarget.value as InstanceRole)}
                      class="shrink-0"
                    >
                      <option value="owner" disabled={roleOptionDisabled(u, 'owner')}>Owner</option>
                      <option value="manager" disabled={roleOptionDisabled(u, 'manager')}
                        >Manager</option
                      >
                      <option value="member" disabled={roleOptionDisabled(u, 'member')}
                        >Member</option
                      >
                    </NativeSelect>
                  {:else}
                    <Badge
                      variant="outline"
                      class={u.instance_role === 'owner'
                        ? 'text-primary shrink-0 capitalize'
                        : 'shrink-0 capitalize'}
                    >
                      {u.instance_role}
                    </Badge>
                  {/if}
                  {#if u.status === 'pending'}
                    <div class="flex shrink-0 items-center gap-2">
                      <Button
                        variant="default"
                        size="sm"
                        disabled={busyUserId === u.id}
                        onclick={() => approveUser(u.id)}
                      >
                        <UserCheck class="size-4" />
                        Approve
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busyUserId === u.id || isSelf}
                        onclick={() => rejectUser(u.id, u.email)}
                      >
                        <Trash2 class="size-4" />
                        Reject
                      </Button>
                    </div>
                  {:else if canDelete(u, isSelf)}
                    <Button
                      variant="ghost"
                      size="icon"
                      class="text-destructive hover:text-destructive size-8 shrink-0"
                      title="Delete account"
                      aria-label={`Delete account ${u.email}`}
                      disabled={busyUserId === u.id}
                      onclick={() => deleteUser(u.id, u.email)}
                    >
                      <Trash2 class="size-4" />
                    </Button>
                  {:else}
                    <span class="size-8 shrink-0" aria-hidden="true"></span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </Card.Content>
      </Card.Root>
    </Tabs.Content>

    <!-- Teams overview -->
    <Tabs.Content value="teams">
      {#if data.teams.length === 0}
        <p class="text-muted-foreground text-sm">No teams yet.</p>
      {:else}
        <TeamList teams={data.teams} />
      {/if}
    </Tabs.Content>
  </Tabs.Root>

  <div class="flex items-center justify-center gap-2 border-t pt-4 select-none">
    <span class="text-muted-foreground/75 text-xs">v{version}</span>
    <span class="text-muted-foreground/50 text-xs">|</span>
    <a
      href="https://github.com/lebe-dev/soika"
      target="_blank"
      rel="noopener noreferrer"
      class="text-muted-foreground/75 hover:text-foreground flex items-center gap-1 text-xs underline transition-colors"
    >
      GitHub
      <ExternalLink class="size-2.5" />
    </a>
  </div>
</div>
