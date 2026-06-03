<script lang="ts">
  import { untrack } from 'svelte';
  import { version } from '$app/environment';
  import { invalidateAll } from '$app/navigation';
  import { api, errorMessage, type UpdateSettingsRequest } from '$lib/api';
  import { formatDate } from '$lib/format';
  import { authStore } from '$lib/stores/auth.svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Tabs from '$lib/components/ui/tabs';
  import * as Table from '$lib/components/ui/table';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { toast } from '$lib/components/ui/sonner';
  import Shield from '@lucide/svelte/icons/shield';
  import Check from '@lucide/svelte/icons/check';
  import Mail from '@lucide/svelte/icons/mail';
  import MailX from '@lucide/svelte/icons/mail-x';
  import type { PageData } from './$types';

  // Admin — service settings, users, and teams overview.
  // Visible only to the instance admin (guarded in +page.ts).
  let { data }: { data: PageData } = $props();

  // Service-settings form state, seeded from the loaded settings. After a
  // successful save we `invalidateAll()`; `reset()` then re-syncs the form to
  // the freshly persisted values read from `data.settings`.
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
    } finally {
      saving = false;
    }
  }

  function reset() {
    orgName = data.settings.org_name;
    allowSignup = data.settings.allow_signup;
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Admin</h1>
    <p class="text-muted-foreground text-sm">Instance-wide settings, users, and teams.</p>
  </div>

  <Tabs.Root value="settings">
    <Tabs.List>
      <Tabs.Trigger value="settings">Service settings</Tabs.Trigger>
      <Tabs.Trigger value="users">Users ({data.users.length})</Tabs.Trigger>
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

      <Card.Root>
        <Card.Header>
          <Card.Title>About</Card.Title>
          <Card.Description>Build information for this instance.</Card.Description>
        </Card.Header>
        <Card.Content>
          <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
            <dt class="text-muted-foreground">Version</dt>
            <dd class="font-mono">soika v{version}</dd>
          </dl>
        </Card.Content>
      </Card.Root>

      <div class="flex items-center gap-2">
        <Button onclick={save} disabled={!dirty || saving}>
          {saving ? 'Saving…' : 'Save changes'}
        </Button>
        <Button variant="ghost" onclick={reset} disabled={!dirty || saving}>Reset</Button>
      </div>
    </Tabs.Content>

    <!-- Users overview -->
    <Tabs.Content value="users">
      <Card.Root>
        <Card.Header>
          <Card.Title>Users</Card.Title>
          <Card.Description>All accounts on this instance.</Card.Description>
        </Card.Header>
        <Card.Content>
          {#if data.users.length === 0}
            <p class="text-muted-foreground text-sm">No users yet.</p>
          {:else}
            <Table.Root>
              <Table.Header>
                <Table.Row>
                  <Table.Head>Name</Table.Head>
                  <Table.Head>Email</Table.Head>
                  <Table.Head>Role</Table.Head>
                  <Table.Head>Notifications</Table.Head>
                  <Table.Head>Joined</Table.Head>
                </Table.Row>
              </Table.Header>
              <Table.Body>
                {#each data.users as u (u.id)}
                  <Table.Row>
                    <Table.Cell class="font-medium">
                      {u.display_name}
                      {#if authStore.user && u.id === authStore.user.id}
                        <span class="text-muted-foreground ml-1 text-xs">(you)</span>
                      {/if}
                    </Table.Cell>
                    <Table.Cell class="text-muted-foreground">{u.email}</Table.Cell>
                    <Table.Cell>
                      {#if u.is_admin}
                        <Badge variant="default">
                          <Shield class="size-3" />
                          Admin
                        </Badge>
                      {:else}
                        <Badge variant="outline">Member</Badge>
                      {/if}
                    </Table.Cell>
                    <Table.Cell>
                      <span class="text-muted-foreground text-sm">
                        {u.notifications_enabled ? 'On' : 'Off'}
                      </span>
                    </Table.Cell>
                    <Table.Cell class="text-muted-foreground">{formatDate(u.created_at)}</Table.Cell
                    >
                  </Table.Row>
                {/each}
              </Table.Body>
            </Table.Root>
          {/if}
        </Card.Content>
      </Card.Root>
    </Tabs.Content>

    <!-- Teams overview -->
    <Tabs.Content value="teams">
      <Card.Root>
        <Card.Header>
          <Card.Title>Teams</Card.Title>
          <Card.Description>Teams and their membership across the instance.</Card.Description>
        </Card.Header>
        <Card.Content>
          {#if data.teams.length === 0}
            <p class="text-muted-foreground text-sm">No teams yet.</p>
          {:else}
            <Table.Root>
              <Table.Header>
                <Table.Row>
                  <Table.Head>Team</Table.Head>
                  <Table.Head class="text-right">Members</Table.Head>
                  <Table.Head class="text-right">Projects</Table.Head>
                </Table.Row>
              </Table.Header>
              <Table.Body>
                {#each data.teams as t (t.id)}
                  <Table.Row>
                    <Table.Cell class="font-medium">
                      <a href={`/teams/${t.id}`} class="hover:underline">{t.name}</a>
                    </Table.Cell>
                    <Table.Cell class="text-muted-foreground text-right"
                      >{t.member_count}</Table.Cell
                    >
                    <Table.Cell class="text-muted-foreground text-right"
                      >{t.project_count}</Table.Cell
                    >
                  </Table.Row>
                {/each}
              </Table.Body>
            </Table.Root>
          {/if}
        </Card.Content>
      </Card.Root>
    </Tabs.Content>
  </Tabs.Root>
</div>
