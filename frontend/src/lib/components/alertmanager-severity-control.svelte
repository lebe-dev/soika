<script lang="ts">
  import { untrack } from 'svelte';
  import { invalidateAll } from '$app/navigation';
  import {
    notifications,
    errorMessage,
    type AlertSeverity,
    type ProjectAlertSeverity
  } from '$lib/api';
  import { t } from '$lib/i18n';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { NativeSelect } from '$lib/components/ui/native-select';
  import { Button } from '$lib/components/ui/button';
  import { toast } from '$lib/components/ui/sonner';

  let {
    view,
    projectId,
    readonly = false,
    managed = false
  }: {
    view: ProjectAlertSeverity;
    projectId: string;
    readonly?: boolean;
    managed?: boolean;
  } = $props();
  let severity = $state<AlertSeverity | ''>(untrack(() => view.severity ?? ''));
  let saving = $state(false);
  let error = $state('');
  const dirty = $derived(severity !== (view.severity ?? ''));

  async function save(event: SubmitEvent) {
    event.preventDefault();
    saving = true;
    error = '';
    try {
      view = await notifications.setProjectSeverity(projectId, severity || null);
      severity = view.severity ?? '';
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

<Card.Root>
  <Card.Header>
    <Card.Title>{$t('alertmanager_severity_title')}</Card.Title>
    <Card.Description>{$t('alertmanager_severity_help')}</Card.Description>
  </Card.Header>
  <Card.Content>
    <form onsubmit={save} class="space-y-3">
      {#if managed}<p class="text-muted-foreground text-sm">
          {$t('alertmanager_severity_yaml')}
        </p>{/if}
      <label class="block space-y-2 text-sm">
        <span>{$t('alertmanager_severity_title')}</span>
        <NativeSelect bind:value={severity} disabled={readonly || saving}>
          <option value="">{$t('alertmanager_severity_default')}</option>
          <option value="critical">critical</option>
          <option value="warning">warning</option>
          <option value="info">info</option>
        </NativeSelect>
      </label>
      {#if error}<p role="alert" class="text-destructive text-sm">{error}</p>{/if}
      {#if !readonly}<Button type="submit" disabled={saving || !dirty}
          >{saving ? $t('saving') : $t('alertmanager_severity_save')}</Button
        >{/if}
    </form>
  </Card.Content>
</Card.Root>
