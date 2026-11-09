<script lang="ts">
  import { untrack } from 'svelte';
  import { invalidateAll } from '$app/navigation';
  import {
    notifications,
    errorMessage,
    type AlertSeverity,
    type SeverityMode,
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
  const initialMode = () => view.mode ?? (projectId && !view.severity ? '' : 'fixed');
  let mode = $state<SeverityMode | ''>(untrack(initialMode));
  let saving = $state(false);
  let error = $state('');
  const dirty = $derived(severity !== (view.severity ?? '') || mode !== initialMode());

  async function save(event: SubmitEvent) {
    event.preventDefault();
    saving = true;
    error = '';
    try {
      view = projectId
        ? await notifications.setProjectSeverity(
            projectId,
            mode ? severity || null : null,
            mode || null
          )
        : await notifications.setDefaultSeverity(severity || null, mode || 'fixed');
      severity = view.severity ?? '';
      mode = initialMode();
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
      <label class="block space-y-2 text-sm">
        <span>{$t('alertmanager_severity_mode')}</span>
        <NativeSelect
          bind:value={mode}
          data-managed-readonly={managed || undefined}
          disabled={readonly || saving}
        >
          {#if projectId}<option value="">{$t('alertmanager_severity_default')}</option>{/if}
          <option value="fixed">{$t('alertmanager_severity_mode_fixed')}</option>
          <option value="event">{$t('alertmanager_severity_mode_event')}</option>
        </NativeSelect>
      </label>
      {#if mode === 'event'}<p class="text-muted-foreground text-sm">
          {$t('alertmanager_severity_mapping')}
        </p>{/if}
      <label class="block space-y-2 text-sm">
        <span
          >{$t(
            mode === 'event'
              ? 'alertmanager_severity_fallback'
              : projectId
                ? 'alertmanager_severity_title'
                : 'alertmanager_default_severity_title'
          )}</span
        >
        <NativeSelect
          bind:value={severity}
          data-managed-readonly={managed || undefined}
          disabled={readonly || saving || (!!projectId && !mode)}
        >
          <option value=""
            >{$t(
              projectId
                ? 'alertmanager_severity_default'
                : mode === 'event'
                  ? 'alertmanager_severity_fallback_critical'
                  : 'alertmanager_default_severity_template'
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
