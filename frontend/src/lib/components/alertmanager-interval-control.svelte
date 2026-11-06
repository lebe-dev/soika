<script lang="ts">
  import { untrack } from 'svelte';
  import { invalidateAll } from '$app/navigation';
  import { notifications, errorMessage, type AlertmanagerInterval } from '$lib/api';
  import { t } from '$lib/i18n';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import { toast } from '$lib/components/ui/sonner';

  let {
    interval,
    projectId,
    readonly = false,
    managed = false,
    managedBy = 'yaml'
  }: {
    interval: AlertmanagerInterval;
    projectId?: string;
    readonly?: boolean;
    managed?: boolean;
    managedBy?: 'yaml' | 'env';
  } = $props();
  let seconds = $state<number | undefined>(untrack(() => interval.effective_interval_seconds));
  let inherit = $state(untrack(() => Boolean(projectId && interval.interval_seconds === null)));
  let saving = $state(false);
  let error = $state('');

  async function save(event: SubmitEvent) {
    event.preventDefault();
    saving = true;
    error = '';
    try {
      const result = projectId
        ? await notifications.setProjectInterval(projectId, inherit ? null : seconds!)
        : await notifications.setInterval(seconds!);
      interval = result;
      seconds = result.effective_interval_seconds;
      inherit = Boolean(projectId && result.interval_seconds === null);
      toast.success($t('notifications_saved'));
      await invalidateAll();
    } catch (err) {
      error = errorMessage(err, $t('failed_to_save_settings'));
      reportUnexpected(err);
    } finally {
      saving = false;
    }
  }
</script>

<Card.Root class={projectId ? 'contents' : undefined}>
  <Card.Header>
    <Card.Title>{$t('alertmanager_interval_title')}</Card.Title>
    <Card.Description>{$t('alertmanager_interval_help')}</Card.Description>
  </Card.Header>
  <Card.Content>
    <form onsubmit={save} class="space-y-3">
      {#if managed && !projectId}<p class="text-muted-foreground text-sm">
          {$t(managedBy === 'env' ? 'alertmanager_defaults_env' : 'alertmanager_interval_yaml')}
        </p>{/if}
      {#if projectId}
        <label class="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            bind:checked={inherit}
            data-managed-readonly={readonly || undefined}
            disabled={readonly || saving}
          />
          {$t('alertmanager_interval_inherit')}
        </label>
      {/if}
      <label class="block space-y-2 text-sm">
        <span>{$t('alertmanager_interval_title')}</span>
        <Input
          type="number"
          min={10}
          max={86400}
          step={1}
          bind:value={seconds}
          data-managed-readonly={readonly || undefined}
          disabled={readonly || saving || inherit}
          required
        />
      </label>
      <p class="text-muted-foreground text-sm">
        {$t('alertmanager_interval_effective', {
          values: { seconds: interval.effective_interval_seconds }
        })}
      </p>
      {#if error}<p role="alert" class="text-destructive text-sm">{error}</p>{/if}
      {#if !readonly}<Button type="submit" disabled={saving}
          >{saving ? $t('saving') : $t('alertmanager_interval_save')}</Button
        >{/if}
    </form>
  </Card.Content>
</Card.Root>
