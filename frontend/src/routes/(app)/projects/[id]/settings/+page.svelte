<script lang="ts">
  import { untrack } from 'svelte';
  import { goto, invalidateAll } from '$app/navigation';
  import { projects, errorMessage, type TagMuteRule } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import { toast } from '$lib/components/ui/sonner';
  import TagMuteRuleDialog from '$lib/components/tag-mute-rule-dialog.svelte';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Plus from '@lucide/svelte/icons/plus';

  // Project Settings: retention, project mute, webhook, and tag-mute rules.
  // Admin actions are gated by the caller's *effective* project role (resolved
  // from team membership in the parent layout load); non-admins see read-only
  // sections (the API also enforces this). Project membership and invites are
  // managed at the team level — see the team detail page. The project comes from
  // the parent layout load (fetched once on project open); mutations update
  // local state and `invalidateAll()` re-runs that load.
  let { data }: { data: PageData } = $props();

  // The project comes from the parent layout load; mutations call
  // `invalidateAll()`, which re-runs that load and flows back through here.
  const project = $derived(data.project);

  // The caller administers this project when their effective role is `admin`
  // (instance Owner/Manager, or a Team Admin of the owning team). Resolved in
  // the layout load; the API enforces the real check on every write.
  const isAdmin = $derived(data.effectiveRole === 'admin');

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

  <!-- Members & invites are managed at the team level -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Members</Card.Title>
      <Card.Description>
        Access to this project comes from membership in its owning team. Add members, set team
        roles, and create invites from the team page.
      </Card.Description>
    </Card.Header>
    <Card.Content>
      <Button variant="outline" href={`/teams/${project.team_id}`}>Manage team</Button>
    </Card.Content>
  </Card.Root>

  <!-- Danger zone (admin only) -->
  {#if isAdmin}
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
