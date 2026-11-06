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
    projectId?: string;
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
      view = projectId
        ? await notifications.setProjectSeverity(projectId, severity || null)
        : await notifications.setDefaultSeverity(severity || null);
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

<Card.Root class={projectId ? 'contents' : undefined}>
  <Card.Header>
    <Card.Title
      >{$t(
        projectId ? 'alertmanager_severity_title' : 'alertmanager_default_severity_title'
      )}</Card.Title
    >
    <Card.Description
      >{$t(
        projectId ? 'alertmanager_severity_help' : 'alertmanager_default_severity_help'
      )}</Card.Description
    >
  </Card.Header>
  <Card.Content>
    <form onsubmit={save} class="space-y-3">
      {#if managed && !projectId}<p class="text-muted-foreground text-sm">
          {$t(projectId ? 'alertmanager_severity_yaml' : 'alertmanager_defaults_env')}
        </p>{/if}
      <label class="block space-y-2 text-sm">
        <span
          >{$t(
            projectId ? 'alertmanager_severity_title' : 'alertmanager_default_severity_title'
          )}</span
        >
        <NativeSelect
          bind:value={severity}
          data-managed-readonly={readonly || undefined}
          disabled={readonly || saving}
        >
          <option value=""
            >{$t(
              projectId ? 'alertmanager_severity_default' : 'alertmanager_default_severity_template'
            )}</option
          >
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
