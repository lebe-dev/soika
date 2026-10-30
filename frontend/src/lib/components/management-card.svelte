<script lang="ts">
  import { t } from '$lib/i18n';
  import * as Card from '$lib/components/ui/card';
  import type { ManagementStatus } from '$lib/api';
  let { management }: { management: ManagementStatus } = $props();
</script>

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
      <dt>{$t('projects')} <code>PROJECT_MANAGEMENT_SOURCE</code></dt>
      <dd>{management.settings.project_source === 'ui' ? 'UI' : $t('configuration_file')}</dd>
      <dt>{$t('teams')} <code>TEAM_MANAGEMENT_SOURCE</code></dt>
      <dd>{management.settings.team_source === 'ui' ? 'UI' : $t('configuration_file')}</dd>
      <dt>{$t('notifications_tab')} <code>NOTIFICATION_MANAGEMENT_SOURCE</code></dt>
      <dd>{management.settings.notification_source === 'ui' ? 'UI' : $t('configuration_file')}</dd>
      <dt>{$t('configuration_file')}</dt>
      <dd>{management.settings.path}</dd>
      <dt>{$t('status')}</dt>
      <dd>
        {management.report.loaded
          ? management.report.default_file
            ? $t('default_configuration_file')
            : $t('loaded')
          : $t('not_used')}
      </dd>
      <dt>{$t('last_sync')}</dt>
      <dd>{management.report.synced_at ?? '—'}</dd>
      <dt>{$t('projects_in_file')}</dt>
      <dd>{management.report.projects_in_file}</dd>
      <dt>{$t('teams_in_file')}</dt>
      <dd>{management.report.teams_in_file}</dd>
      <dt>{$t('orphan_policy')}</dt>
      <dd>
        {management.settings.project_orphan_policy} / {management.settings.team_orphan_policy}
      </dd>
      <dt>{$t('sync_result')}</dt>
      <dd>
        +{management.report.created} / ~{management.report.updated} / ={management.report.unchanged} /
        ↩{management.report.restored} / !{management.report.orphaned} / −{management.report.deleted}
      </dd>
      <dt>{$t('not_signed_in_yet')}</dt>
      <dd>{management.report.pending_grants}</dd>
      <dt>{$t('local_account_grants')}</dt>
      <dd>{management.report.local_grants}</dd>
      <dt>{$t('orphaned_entities')}</dt>
      <dd>{management.report.orphaned_projects ?? 0} / {management.report.orphaned_teams ?? 0}</dd>
    </dl>
    {#if management.settings.project_orphan_policy === 'delete' || management.settings.team_orphan_policy === 'delete'}<p
        class="text-destructive"
      >
        {$t('delete_orphans_warning')}
      </p>{/if}
    {#if management.settings.team_source === 'yaml'}
      <p>{$t('signup_invites_disabled')}</p>
      {#if !management.oauth_enabled}<p class="text-destructive">
          {$t('teams_without_sso_warning')}
        </p>{/if}
    {/if}
    {#each management.report.local_accounts ?? [] as account}<p>
        {account.email} · {$t(account.role)}
      </p>{/each}
  </Card.Content>
</Card.Root>
