<script lang="ts">
  import { untrack } from 'svelte';
  import { goto, invalidateAll } from '$app/navigation';
  import {
    projects,
    errorMessage,
    type Invite,
    type Project,
    type ProjectMember,
    type Role,
    type TagMuteRule
  } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import * as Table from '$lib/components/ui/table';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import { toast } from '$lib/components/ui/sonner';
  import CopyField from '$lib/components/copy-field.svelte';
  import TagMuteRuleDialog from '$lib/components/tag-mute-rule-dialog.svelte';
  import { formatRelative } from '$lib/format';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import UserPlus from '@lucide/svelte/icons/user-plus';
  import Plus from '@lucide/svelte/icons/plus';

  // Project Settings: retention, project mute, members list, and
  // invite generation with a copyable link. Admin actions are gated; non-admins
  // see read-only sections (the API also enforces this). Project, members and
  // invites all come from the parent layout load (fetched once on project open);
  // mutations update local state and `invalidateAll()` re-runs that load.
  let { data }: { data: PageData } = $props();

  // The project comes from the parent layout load; mutations call
  // `invalidateAll()`, which re-runs that load and flows back through here.
  const project = $derived(data.project);

  // Members and invites are seeded once from the load and then edited locally on
  // add/remove for an immediate response. They are the authoritative view after a
  // mutation, so we seed (not mirror): `untrack` makes the one-time read explicit
  // and avoids a re-syncing effect that would clobber the optimistic local edits.
  let members = $state<ProjectMember[]>(untrack(() => data.members));

  let invites = $state<Invite[]>(untrack(() => data.invites));

  // The current user is a project admin if they are the instance admin or hold
  // the admin role here. We infer the latter from the members list.
  const isAdmin = $derived(
    authStore.isAdmin || members.some((m) => m.user_id === authStore.user?.id && m.role === 'admin')
  );

  // --- General: name + retention -----------------------------------------
  // Editable fields are seeded once from the project and re-seeded by
  // `resetGeneral()` after a save reconciles the load. Keeping them as plain
  // `$state` (not an effect mirror) preserves in-progress edits.
  let name = $state(untrack(() => data.project.name));
  let retention = $state<number>(untrack(() => data.project.retention_events));
  let retentionDays = $state<number>(untrack(() => data.project.retention_days));
  let savingGeneral = $state(false);
  const generalDirty = $derived(
    name !== project.name ||
      retention !== project.retention_events ||
      retentionDays !== project.retention_days
  );

  function resetGeneral() {
    name = project.name;
    retention = project.retention_events;
    retentionDays = project.retention_days;
  }

  async function saveGeneral(event: SubmitEvent) {
    event.preventDefault();
    savingGeneral = true;
    try {
      await projects.update(project.id, {
        name: name.trim(),
        retention_events: retention,
        retention_days: retentionDays
      });
      // Re-run the parent layout load so the project header (name/slug) and
      // sibling pages reflect the change without a manual reload; `project`
      // derives from that fresh data.
      await invalidateAll();
      resetGeneral();
      toast.success('Project updated');
    } catch (err) {
      toast.error(errorMessage(err, 'Update failed'));
      reportUnexpected(err);
    } finally {
      savingGeneral = false;
    }
  }

  // --- Mute ----------------------------------------------------------------
  let mutating = $state(false);
  async function toggleMute() {
    mutating = true;
    const nextMuted = !project.muted;
    try {
      await projects.mute(project.id, nextMuted);
      await invalidateAll();
      toast.success(nextMuted ? 'Project muted' : 'Project unmuted');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to update mute'));
      reportUnexpected(err);
    } finally {
      mutating = false;
    }
  }

  // --- Tag-mute rules ------------------------------------------------------
  // Project-level rules that suppress notifications for events whose tags match.
  // Manageable by any project member (same bar as resolve/mute).
  let muteRules = $state<TagMuteRule[]>(untrack(() => data.muteRules));
  let ruleDialogOpen = $state(false);
  let deletingRule = $state<string | null>(null);

  function onRuleCreated(rule: TagMuteRule) {
    muteRules = [rule, ...muteRules];
  }

  async function deleteRule(ruleId: string) {
    deletingRule = ruleId;
    try {
      await projects.deleteMuteRule(project.id, ruleId);
      muteRules = muteRules.filter((r) => r.id !== ruleId);
      toast.success('Rule removed');
    } catch (err) {
      toast.error(errorMessage(err, 'Could not remove rule'));
      reportUnexpected(err);
    } finally {
      deletingRule = null;
    }
  }

  // --- Webhook -------------------------------------------------
  // Same pattern as the general form: a locally-edited field, re-seeded by
  // `resetWebhook()` once a save reconciles the load.
  let webhookUrl = $state(untrack(() => data.project.webhook_url ?? ''));
  let savingWebhook = $state(false);
  const webhookDirty = $derived(webhookUrl !== (project.webhook_url ?? ''));

  function resetWebhook() {
    webhookUrl = project.webhook_url ?? '';
  }

  async function saveWebhook(event: SubmitEvent) {
    event.preventDefault();
    savingWebhook = true;
    try {
      await projects.update(project.id, { webhook_url: webhookUrl.trim() });
      await invalidateAll();
      resetWebhook();
      toast.success('Webhook updated');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to update webhook'));
      reportUnexpected(err);
    } finally {
      savingWebhook = false;
    }
  }

  // --- Members -------------------------------------------------------------
  async function removeMember(member: ProjectMember) {
    if (!confirm(`Remove ${member.display_name} from this project?`)) return;
    try {
      await projects.removeMember(project.id, member.user_id);
      members = members.filter((m) => m.user_id !== member.user_id);
      toast.success(`Removed ${member.display_name}`);
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to remove member'));
      reportUnexpected(err);
    }
  }

  // --- Invites -------------------------------------------------------------
  let inviteEmail = $state('');
  let inviteRole = $state<Role>('member');
  let creatingInvite = $state(false);

  async function createInvite(event: SubmitEvent) {
    event.preventDefault();
    creatingInvite = true;
    try {
      const email = inviteEmail.trim();
      const invite = await projects.createInvite(project.id, {
        role: inviteRole,
        email: email.length > 0 ? email : undefined
      });
      invites = [invite, ...invites];
      inviteEmail = '';
      toast.success(invite.email_sent ? 'Invite created and emailed' : 'Invite link created');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to create invite'));
      reportUnexpected(err);
    } finally {
      creatingInvite = false;
    }
  }

  async function revokeInvite(invite: Invite) {
    try {
      await projects.revokeInvite(project.id, invite.token);
      invites = invites.filter((i) => i.token !== invite.token);
      toast.success('Invite revoked');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to revoke invite'));
      reportUnexpected(err);
    }
  }

  // Pending = not yet accepted. Accepted invites are kept out of the list.
  const pendingInvites = $derived(invites.filter((i) => !i.accepted_at));

  // --- Danger zone ---------------------------------------------------------
  let deleting = $state(false);
  async function deleteProject() {
    if (!confirm(`Delete project "${project.name}"? This cannot be undone.`)) return;
    deleting = true;
    try {
      await projects.remove(project.id);
      toast.success('Project deleted');
      await invalidateAll();
      await goto('/');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to delete project'));
      reportUnexpected(err);
      deleting = false;
    }
  }
</script>

<PageTitle title={`${project.name} · Settings`} />

<div class="space-y-6">
  <!-- General settings -->
  <Card.Root>
    <Card.Header>
      <Card.Title>General</Card.Title>
      <Card.Description>Project name and event retention (by count and age).</Card.Description>
    </Card.Header>
    <form onsubmit={saveGeneral} class="contents">
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <label for="name" class="text-sm font-medium">Name</label>
          <Input id="name" bind:value={name} disabled={!isAdmin} required />
        </div>
        <div class="space-y-2">
          <label for="retention" class="text-sm font-medium">Retention (max events)</label>
          <Input
            id="retention"
            type="number"
            min={1}
            bind:value={retention}
            disabled={!isAdmin}
            required
          />
          <p class="text-muted-foreground text-xs">
            Keep at most this many recent events; older events are pruned. Issue counters are
            preserved.
          </p>
        </div>
        <div class="space-y-2">
          <label for="retention-days" class="text-sm font-medium">Retention (days)</label>
          <Input
            id="retention-days"
            type="number"
            min={0}
            bind:value={retentionDays}
            disabled={!isAdmin}
          />
          <p class="text-muted-foreground text-xs">
            0 = keep events regardless of age; otherwise events older than this many days are
            pruned. Issue counters are preserved.
          </p>
        </div>
      </Card.Content>
      {#if isAdmin}
        <Card.Footer>
          <Button type="submit" disabled={savingGeneral || !generalDirty}>
            {savingGeneral ? 'Saving…' : 'Save changes'}
          </Button>
        </Card.Footer>
      {/if}
    </form>
  </Card.Root>

  <!-- Mute -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Notifications</Card.Title>
      <Card.Description>
        Muting suppresses notifications for all issues in this project. Ingestion and counting
        continue.
      </Card.Description>
    </Card.Header>
    <Card.Content class="space-y-6">
      <div class="flex items-center justify-between gap-4">
        <div class="text-sm">
          Status:
          {#if project.muted}
            <Badge variant="secondary">Muted</Badge>
          {:else}
            <Badge variant="outline">Active</Badge>
          {/if}
        </div>
        {#if isAdmin}
          <Button variant="outline" disabled={mutating} onclick={toggleMute}>
            {project.muted ? 'Unmute project' : 'Mute project'}
          </Button>
        {/if}
      </div>

      <form onsubmit={saveWebhook} class="space-y-2">
        <label for="webhook-url" class="text-sm font-medium">Webhook URL</label>
        <Input
          id="webhook-url"
          type="url"
          placeholder="https://..."
          bind:value={webhookUrl}
          disabled={!isAdmin}
        />
        <p class="text-muted-foreground text-xs">
          POST new-issue and regression notifications as JSON to this URL. Leave empty to disable.
        </p>
        {#if isAdmin}
          <Button type="submit" variant="outline" disabled={savingWebhook || !webhookDirty}>
            {savingWebhook ? 'Saving…' : 'Save webhook'}
          </Button>
        {/if}
      </form>
    </Card.Content>
  </Card.Root>

  <!-- Tag-mute rules -->
  <Card.Root>
    <Card.Header>
      <div class="flex items-start justify-between gap-4">
        <div>
          <Card.Title>Mute by tags</Card.Title>
          <Card.Description>
            Suppress notifications for events whose tags match a rule. All pairs in a rule must
            match (AND). Events are still ingested and counted.
          </Card.Description>
        </div>
        <Button
          variant="outline"
          class="gap-2 whitespace-nowrap"
          onclick={() => (ruleDialogOpen = true)}
        >
          <Plus class="size-4" />
          Add rule
        </Button>
      </div>
    </Card.Header>
    <Card.Content>
      {#if muteRules.length === 0}
        <p class="text-muted-foreground text-sm">No tag-mute rules yet.</p>
      {:else}
        <ul class="divide-border divide-y">
          {#each muteRules as rule (rule.id)}
            <li class="flex items-start justify-between gap-4 py-3 first:pt-0 last:pb-0">
              <div class="min-w-0 space-y-1.5">
                {#if rule.name}
                  <div class="text-sm font-medium">{rule.name}</div>
                {/if}
                <div class="flex flex-wrap gap-1.5">
                  {#each rule.tags as tag (tag.key)}
                    <Badge variant="secondary" class="font-mono text-xs">
                      {tag.key}={tag.value}
                    </Badge>
                  {/each}
                </div>
              </div>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Remove rule"
                disabled={deletingRule === rule.id}
                onclick={() => deleteRule(rule.id)}
              >
                <Trash2 class="size-4" />
              </Button>
            </li>
          {/each}
        </ul>
      {/if}
    </Card.Content>
  </Card.Root>

  <!-- Members -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Members</Card.Title>
      <Card.Description>People with access to this project.</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if members.length === 0}
        <p class="text-muted-foreground text-sm">No members to display.</p>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Member</Table.Head>
              <Table.Head>Role</Table.Head>
              {#if isAdmin}
                <Table.Head class="w-10"></Table.Head>
              {/if}
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each members as member (member.user_id)}
              <Table.Row>
                <Table.Cell>
                  <div class="font-medium">{member.display_name}</div>
                  <div class="text-muted-foreground text-xs">{member.email}</div>
                </Table.Cell>
                <Table.Cell>
                  {#if member.role === 'admin'}
                    <span class="text-primary text-sm font-medium">admin</span>
                  {:else}
                    <Badge variant="outline">{member.role}</Badge>
                  {/if}
                </Table.Cell>
                {#if isAdmin}
                  <Table.Cell>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-8"
                      onclick={() => removeMember(member)}
                      aria-label={`Remove ${member.display_name}`}
                    >
                      <Trash2 class="text-destructive size-4" />
                    </Button>
                  </Table.Cell>
                {/if}
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </Card.Content>
  </Card.Root>

  <!-- Invites (admin only) -->
  {#if isAdmin}
    <Card.Root>
      <Card.Header>
        <Card.Title>Invites</Card.Title>
        <Card.Description>
          Generate an invite link to add someone to this project. The link works even without email;
          if SMTP is configured and you provide an address, it is also emailed.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        <form class="flex flex-wrap items-end gap-3" onsubmit={createInvite}>
          <div class="min-w-48 flex-1 space-y-2">
            <label for="invite-email" class="text-sm font-medium">Email (optional)</label>
            <Input
              id="invite-email"
              type="email"
              placeholder="teammate@example.com"
              bind:value={inviteEmail}
            />
          </div>
          <div class="space-y-2">
            <label for="invite-role" class="text-sm font-medium">Role</label>
            <select
              id="invite-role"
              bind:value={inviteRole}
              class="border-input dark:bg-input/30 focus-visible:border-ring focus-visible:ring-ring/50 h-8 rounded-lg border bg-transparent px-2.5 text-sm transition-colors outline-none focus-visible:ring-3"
            >
              <option value="member">Member</option>
              <option value="admin">Admin</option>
            </select>
          </div>
          <Button type="submit" disabled={creatingInvite}>
            <UserPlus class="size-4" />
            {creatingInvite ? 'Creating…' : 'Create invite'}
          </Button>
        </form>

        {#if pendingInvites.length > 0}
          <div class="space-y-3">
            {#each pendingInvites as invite (invite.token)}
              <div class="space-y-2 rounded-md border p-3">
                <div class="flex items-center justify-between gap-2">
                  <div class="flex items-center gap-2 text-sm">
                    {#if invite.role === 'admin'}
                      <span class="text-primary text-sm font-medium">admin</span>
                    {:else}
                      <Badge variant="outline">{invite.role}</Badge>
                    {/if}
                    {#if invite.email}
                      <span>{invite.email}</span>
                    {:else}
                      <span class="text-muted-foreground">Anyone with the link</span>
                    {/if}
                    <span class="text-muted-foreground text-xs"
                      >· expires {formatRelative(invite.expires_at)}</span
                    >
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="size-8"
                    onclick={() => revokeInvite(invite)}
                    aria-label="Revoke invite"
                  >
                    <Trash2 class="text-destructive size-4" />
                  </Button>
                </div>
                <CopyField value={invite.link} label="invite link" />
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-muted-foreground text-sm">No pending invites.</p>
        {/if}
      </Card.Content>
    </Card.Root>

    <!-- Danger zone -->
    <Card.Root class="border-destructive/40">
      <Card.Header>
        <Card.Title class="text-destructive">Danger zone</Card.Title>
        <Card.Description
          >Deleting a project removes its issues and events permanently.</Card.Description
        >
      </Card.Header>
      <Card.Footer>
        <Button variant="destructive" disabled={deleting} onclick={deleteProject}>
          {deleting ? 'Deleting…' : 'Delete project'}
        </Button>
      </Card.Footer>
    </Card.Root>
  {/if}
</div>

<TagMuteRuleDialog bind:open={ruleDialogOpen} projectId={project.id} onCreated={onRuleCreated} />
