<script lang="ts">
  import { t } from '$lib/i18n';
  import Lock from '@lucide/svelte/icons/lock';

  // One shared notice for the admin area: which parts of the instance are
  // owned by the deployment configuration and therefore read-only here.
  let {
    projectSource,
    teamSource,
    serviceSource,
    notificationSource
  }: {
    projectSource?: 'ui' | 'yaml';
    teamSource?: 'ui' | 'yaml';
    serviceSource?: 'ui' | 'env';
    notificationSource?: 'ui' | 'yaml' | 'env';
  } = $props();

  const areas = $derived.by(() => {
    const list: string[] = [];
    if (serviceSource === 'env' || notificationSource === 'env') {
      list.push($t('managed_area_service'));
    } else if (notificationSource === 'yaml') {
      list.push($t('managed_area_notifications'));
    }
    if (projectSource === 'yaml') list.push($t('managed_area_projects'));
    if (teamSource === 'yaml') list.push($t('managed_area_teams'));
    return list;
  });
</script>

{#if areas.length > 0}
  <div
    role="note"
    class="border-primary/30 bg-primary/5 flex items-start gap-3 rounded-lg border px-4 py-3 text-sm"
  >
    <Lock class="text-primary mt-0.5 size-4 shrink-0" />
    <div class="space-y-0.5">
      <p class="font-medium">{$t('managed_config_title')}</p>
      <p class="text-muted-foreground">
        {$t('managed_config_readonly', { values: { areas: areas.join(', ') } })}
      </p>
    </div>
  </div>
{/if}
