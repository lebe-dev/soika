<script lang="ts">
  import { t } from '$lib/i18n';
  import SquareArrowOutUpRight from '@lucide/svelte/icons/square-arrow-out-up-right';

  // One shared notice for the admin area: which parts of the instance are
  // owned by the deployment configuration and therefore read-only here.
  let {
    projectSource,
    teamSource,
    serviceSource,
    notificationSource,
    deployUrl
  }: {
    projectSource?: 'ui' | 'yaml';
    teamSource?: 'ui' | 'yaml';
    serviceSource?: 'ui' | 'env';
    notificationSource?: 'ui' | 'yaml' | 'env';
    deployUrl?: string | null;
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
    class="border-primary/30 bg-primary/5 space-y-0.5 rounded-lg border px-4 py-3 text-sm"
  >
    <p class="font-medium">
      {$t('managed_config_title_prefix')}
      {#if deployUrl}
        <a
          href={deployUrl}
          target="_blank"
          rel="noopener noreferrer"
          class="inline-flex items-center gap-1 underline underline-offset-4"
        >
          {$t('managed_config_title_link')}
          <SquareArrowOutUpRight class="size-3.5 shrink-0" />
        </a>
      {:else}
        {$t('managed_config_title_link')}
      {/if}
    </p>
    <p class="text-muted-foreground">
      {$t('managed_config_readonly', { values: { areas: areas.join(', ') } })}
    </p>
  </div>
{/if}
