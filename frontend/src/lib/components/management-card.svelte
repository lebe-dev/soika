<script lang="ts">
  import { t } from '$lib/i18n';
  import * as Card from '$lib/components/ui/card';
  import InfoTip from '$lib/components/info-tip.svelte';
  import type { ManagementStatus } from '$lib/api';
  let {
    management,
    invitesEnabled = false
  }: { management: ManagementStatus; invitesEnabled?: boolean } = $props();
</script>

{#snippet term(label: string, tip: string)}
  <dt class="flex items-center gap-1.5">{label}<InfoTip text={$t(tip)} /></dt>
{/snippet}

<Card.Root>
  <Card.Header><Card.Title>{$t('management')}</Card.Title></Card.Header>
  <Card.Content class="space-y-3 text-sm">
    <p>
      {$t(
        'management_summary_' +
          management.settings.project_source +
          '_' +
          management.settings.team_source
      )}
    </p>
    <dl class="grid grid-cols-2 gap-2">
      {@render term($t('projects'), 'tip_projects')}
      <dd>
        {$t(management.settings.project_source === 'ui' ? 'managed_in_ui' : 'configuration_file')}
      </dd>
      {@render term($t('teams'), 'tip_teams')}
      <dd>
        {$t(management.settings.team_source === 'ui' ? 'managed_in_ui' : 'configuration_file')}
      </dd>
      {@render term($t('service_settings'), 'tip_service_settings')}
      <dd>
        {$t(
          management.settings.service_source === 'env' ? 'managed_by_deployment' : 'managed_in_ui'
        )}
      </dd>
      {@render term($t('configuration_file'), 'tip_config_file')}
      <dd>{management.settings.path}</dd>
      {@render term($t('status'), 'tip_status')}
      <dd>
        {management.report.loaded
          ? management.report.default_file
            ? $t('default_configuration_file')
            : $t('loaded')
          : $t('not_used')}
      </dd>
      {@render term($t('last_sync'), 'tip_last_sync')}
      <dd>{management.report.synced_at ?? '—'}</dd>
      {@render term($t('projects_in_file'), 'tip_projects_in_file')}
      <dd>{management.report.projects_in_file}</dd>
      {@render term($t('teams_in_file'), 'tip_teams_in_file')}
      <dd>{management.report.teams_in_file}</dd>
      {@render term($t('orphan_policy'), 'tip_orphan_policy')}
      <dd>
        {management.settings.project_orphan_policy} / {management.settings.team_orphan_policy}
      </dd>
      {@render term($t('sync_result'), 'tip_sync_result')}
      <dd>
        +{management.report.created} / ~{management.report.updated} / ={management.report.unchanged} /
        ↩{management.report.restored} / !{management.report.orphaned} / −{management.report.deleted}
      </dd>
      {@render term($t('not_signed_in_yet'), 'tip_not_signed_in')}
      <dd>{management.report.pending_grants}</dd>
      {@render term($t('local_account_grants'), 'tip_local_grants')}
      <dd>{management.report.local_grants}</dd>
      {@render term($t('orphaned_entities'), 'tip_orphaned_entities')}
      <dd>{management.report.orphaned_projects ?? 0} / {management.report.orphaned_teams ?? 0}</dd>
    </dl>
    {#if management.settings.project_orphan_policy === 'delete' || management.settings.team_orphan_policy === 'delete'}<p
        class="text-destructive"
      >
        {$t('delete_orphans_warning')}
      </p>{/if}
    {#if management.settings.team_source === 'yaml'}
      <p>{$t(invitesEnabled ? 'signup_disabled_invites_enabled' : 'signup_invites_disabled')}</p>
      {#if !management.oauth_enabled}<p class="text-destructive">
          {$t('teams_without_sso_warning')}
        </p>{/if}
    {/if}
    {#each management.report.local_accounts ?? [] as account}<p>
        {account.email} · {$t(account.role)}
      </p>{/each}
  </Card.Content>
</Card.Root>
