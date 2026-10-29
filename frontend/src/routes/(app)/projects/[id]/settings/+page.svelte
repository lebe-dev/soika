<script lang="ts">
  import { t } from '$lib/i18n';
  import { untrack } from 'svelte';
  import { goto, invalidateAll } from '$app/navigation';
  import { projects, errorMessage, type TagMuteRule } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import { NativeSelect } from '$lib/components/ui/native-select';
  import { toast } from '$lib/components/ui/sonner';
  import TagMuteRuleDialog from '$lib/components/tag-mute-rule-dialog.svelte';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';
  import { cn } from '$lib/utils';
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
  const projectsManaged = $derived(data.telemetry?.project_management_source === 'yaml');
  const isAdmin = $derived(data.effectiveRole === 'admin');
  const canEdit = $derived(isAdmin && !projectsManaged);
  const webhooksEnabled = $derived(data.telemetry?.webhooks_enabled ?? false);

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
      toast.success($t('project_updated'));
    } catch (err) {
      toast.error(errorMessage(err, $t('update_failed')));
      reportUnexpected(err);
    } finally {
      savingGeneral = false;
    }
  }

  // --- Owning team (instance Owner/Manager only) ---------------------------
  // Moving a project reassigns who can access it (access derives from the
  // owning team), so it is gated to instance managers. The loader populates
  // `teamOptions` only for them; otherwise the control is hidden. The API
  // enforces the real `require_manager` check.
  const teamOptions = $derived(data.teamOptions);
  const canMoveTeam = $derived(teamOptions.length > 0);
  let selectedTeamId = $state(untrack(() => data.project.team_id));
  let movingTeam = $state(false);
  const teamDirty = $derived(selectedTeamId !== project.team_id);

  async function changeTeam(event: SubmitEvent) {
    event.preventDefault();
    if (!teamDirty) return;
    movingTeam = true;
    try {
      await projects.changeTeam(project.id, { team_id: selectedTeamId });
      await invalidateAll();
      selectedTeamId = project.team_id;
      toast.success($t('project_moved_to_a_different_team'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_move_project')));
      reportUnexpected(err);
    } finally {
      movingTeam = false;
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
      toast.success(nextMuted ? $t('project_muted') : $t('project_unmuted'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_update_mute')));
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
      toast.success($t('rule_removed'));
    } catch (err) {
      toast.error(errorMessage(err, $t('could_not_remove_rule')));
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
      toast.success($t('webhook_updated'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_update_webhook')));
      reportUnexpected(err);
    } finally {
      savingWebhook = false;
    }
  }

  // --- Danger zone ---------------------------------------------------------
  let deleting = $state(false);
  async function deleteProject() {
    if (!confirm($t('delete_project_confirm', { values: { name: project.name } }))) return;
    deleting = true;
    try {
      await projects.remove(project.id);
      toast.success($t('project_deleted'));
      await invalidateAll();
      await goto('/');
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_delete_project')));
      reportUnexpected(err);
      deleting = false;
    }
  }
</script>

<PageTitle title={`${project.name} · ${$t('settings')}`} />
{#if projectsManaged}<p class="text-muted-foreground text-sm">
    {$t('managed_by_configuration')}
  </p>{/if}

<div class="space-y-6">
  <!-- General settings -->
  <Card.Root>
    <Card.Header>
      <Card.Title>{$t('general')}</Card.Title>
      <Card.Description>{$t('project_name_and_event_retention_by_count_and_age')}</Card.Description>
    </Card.Header>
    <form onsubmit={saveGeneral} class="contents">
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <label for="name" class="text-sm font-medium">{$t('name')}</label>
          <Input id="name" bind:value={name} disabled={!canEdit} required />
        </div>
        <div class="space-y-2">
          <label for="retention" class="text-sm font-medium">{$t('retention_max_events')}</label>
          <Input
            id="retention"
            type="number"
            min={1}
            bind:value={retention}
            disabled={!canEdit}
            required
          />
          <p class="text-muted-foreground text-xs">
            {$t('keep_at_most_this_many_recent_events_older_events_are_pruned_issue_counters_are_')}
          </p>
        </div>
        <div class="space-y-2">
          <label for="retention-days" class="text-sm font-medium">{$t('retention_days')}</label>
          <Input
            id="retention-days"
            type="number"
            min={0}
            bind:value={retentionDays}
            disabled={!canEdit}
          />
          <p class="text-muted-foreground text-xs">
            {$t('0_keep_events_regardless_of_age_otherwise_events_older_than_this_many_days_are_p')}
          </p>
        </div>
      </Card.Content>
      {#if canEdit}
        <Card.Footer>
          <Button type="submit" disabled={savingGeneral || !generalDirty}>
            {savingGeneral ? $t('saving') : $t('save_changes')}
          </Button>
        </Card.Footer>
      {/if}
    </form>
  </Card.Root>

  <!-- Mute -->
  <Card.Root class={project.muted ? 'ring-1 ring-orange-500' : undefined}>
    <Card.Header>
      <Card.Title>{$t('notifications')}</Card.Title>
      <Card.Description>
        {$t('muting_suppresses_notifications_for_all_issues_in_this_project_ingestion_and_cou')}
      </Card.Description>
    </Card.Header>
    <Card.Content class="space-y-6">
      <div class="flex items-center justify-between gap-4">
        <div class="text-sm">
          {$t('status_11dc9e')}
          {#if project.muted}
            <Badge variant="secondary">{$t('status_muted')}</Badge>
          {:else}
            <Badge variant="outline">{$t('active')}</Badge>
          {/if}
        </div>
        {#if isAdmin}
          <button
            type="button"
            role="switch"
            aria-checked={!project.muted}
            aria-label={project.muted ? $t('unmute_project') : $t('mute_project')}
            disabled={mutating}
            onclick={toggleMute}
            class={cn(
              'relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full transition-colors',
              'focus-visible:ring-primary/30 focus-visible:ring-offset-background focus-visible:ring-1 focus-visible:ring-offset-2 focus-visible:outline-none',
              'disabled:cursor-not-allowed disabled:opacity-50',
              project.muted ? 'bg-input' : 'bg-primary'
            )}
          >
            <span
              class={cn(
                'bg-background pointer-events-none inline-block size-4 transform rounded-full shadow-sm ring-0 transition-transform',
                project.muted ? 'translate-x-0.5' : 'translate-x-4'
              )}
            ></span>
          </button>
        {/if}
      </div>

      <form
        onsubmit={saveWebhook}
        class={cn('space-y-2', !webhooksEnabled && 'opacity-50 grayscale')}
      >
        {#if !webhooksEnabled}
          <Badge variant="outline">{$t('disabled')}</Badge>
          <p class="text-muted-foreground text-xs">{$t('webhooks_disabled_by_webhooks_enabled')}</p>
        {/if}
        <label for="webhook-url" class="text-sm font-medium">{$t('webhook_url')}</label>
        <Input
          id="webhook-url"
          type="url"
          placeholder="https://..."
          bind:value={webhookUrl}
          disabled={!canEdit || !webhooksEnabled}
        />
        <p class="text-muted-foreground text-xs">
          {$t('post_new_issue_and_regression_notifications_as_json_to_this_url_leave_empty_to_d')}
        </p>
        {#if canEdit}
          <Button
            type="submit"
            variant="outline"
            disabled={!webhooksEnabled || savingWebhook || !webhookDirty}
          >
            {savingWebhook ? $t('saving') : $t('save_webhook')}
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
          <Card.Title>{$t('mute_by_tags')}</Card.Title>
          <Card.Description>
            {$t('suppress_notifications_for_events_whose_tags_match_a_rule_all_pairs_in_a_rule_mu')}
          </Card.Description>
        </div>
        <Button
          variant="outline"
          class="gap-2 whitespace-nowrap"
          onclick={() => (ruleDialogOpen = true)}
        >
          <Plus class="size-4" />
          {$t('add_rule')}
        </Button>
      </div>
    </Card.Header>
    <Card.Content>
      {#if muteRules.length === 0}
        <p class="text-muted-foreground text-sm">{$t('no_tag_mute_rules_yet')}</p>
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
                aria-label={$t('remove_rule')}
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

  <!-- Owning team (instance Owner/Manager only) -->
  {#if canMoveTeam && !projectsManaged}
    <Card.Root>
      <Card.Header>
        <Card.Title>{$t('owning_team')}</Card.Title>
        <Card.Description>
          {$t('move_this_project_to_a_different_team_access_and_admin_rights_follow_the_new_tea')}
        </Card.Description>
      </Card.Header>
      <form onsubmit={changeTeam} class="contents">
        <Card.Content class="space-y-2">
          <label for="owning-team" class="text-sm font-medium">{$t('team')}</label>
          <NativeSelect
            id="owning-team"
            bind:value={selectedTeamId}
            disabled={movingTeam}
            class="w-full"
          >
            {#each teamOptions as team (team.id)}
              <option value={team.id}>{team.name}</option>
            {/each}
          </NativeSelect>
        </Card.Content>
        <Card.Footer>
          <Button type="submit" variant="outline" disabled={movingTeam || !teamDirty}>
            {movingTeam ? $t('moving') : $t('move_project')}
          </Button>
        </Card.Footer>
      </form>
    </Card.Root>
  {/if}

  <!-- Members & invites are managed at the team level -->
  <Card.Root>
    <Card.Header>
      <Card.Title>{$t('members')}</Card.Title>
      <Card.Description>
        {$t('access_to_this_project_comes_from_membership_in_its_owning_team_add_members_set_')}
      </Card.Description>
    </Card.Header>
    <Card.Content>
      <Button variant="outline" href={`/teams/${project.team_id}`}>{$t('manage_team')}</Button>
    </Card.Content>
  </Card.Root>

  <!-- Danger zone (admin only) -->
  {#if canEdit}
    <!-- Danger zone -->
    <Card.Root class="border-destructive/40">
      <Card.Header>
        <Card.Title class="text-destructive">{$t('danger_zone')}</Card.Title>
        <Card.Description
          >{$t('deleting_a_project_removes_its_issues_and_events_permanently')}</Card.Description
        >
      </Card.Header>
      <Card.Footer>
        <Button variant="destructive" disabled={deleting} onclick={deleteProject}>
          {deleting ? $t('deleting') : $t('delete_project')}
        </Button>
      </Card.Footer>
    </Card.Root>
  {/if}
</div>

<TagMuteRuleDialog bind:open={ruleDialogOpen} projectId={project.id} onCreated={onRuleCreated} />
