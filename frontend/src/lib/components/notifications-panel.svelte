<script lang="ts">
  import { untrack } from 'svelte';
  import { invalidateAll } from '$app/navigation';
  import { api, errorMessage, type NotificationView, type NotificationConfig } from '$lib/api';
  import { t, currentLocale } from '$lib/i18n';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { toast } from '$lib/components/ui/sonner';

  let { view }: { view: NotificationView } = $props();
  let enabled = $state(untrack(() => view.config.alertmanager.enabled));
  let url = $state(untrack(() => view.config.alertmanager.url));
  let labels = $state(untrack(() => JSON.stringify(view.config.alertmanager.labels, null, 2)));
  let annotations = $state(
    untrack(() => JSON.stringify(view.config.alertmanager.annotations, null, 2))
  );
  let sendingTest = $state(false);
  let saving = $state(false);
  let previewing = $state(false);
  let preview = $state('');
  let formError = $state('');
  const readonly = $derived(view.source === 'yaml');
  const dirty = $derived(
    enabled !== view.config.alertmanager.enabled ||
      url !== view.config.alertmanager.url ||
      labels !== JSON.stringify(view.config.alertmanager.labels, null, 2) ||
      annotations !== JSON.stringify(view.config.alertmanager.annotations, null, 2)
  );

  function parseMap(value: string): Record<string, string> {
    const map: unknown = JSON.parse(value);
    if (
      !map ||
      Array.isArray(map) ||
      typeof map !== 'object' ||
      Object.values(map).some((v) => typeof v !== 'string')
    )
      throw new Error($t('notifications_string_map'));
    return map as Record<string, string>;
  }
  function config(): NotificationConfig {
    return {
      alertmanager: {
        enabled,
        url: url.trim(),
        labels: parseMap(labels),
        annotations: parseMap(annotations)
      }
    };
  }
  function reset() {
    enabled = view.config.alertmanager.enabled;
    url = view.config.alertmanager.url;
    labels = JSON.stringify(view.config.alertmanager.labels, null, 2);
    annotations = JSON.stringify(view.config.alertmanager.annotations, null, 2);
    preview = '';
    formError = '';
  }
  async function save() {
    let body: NotificationConfig;
    try {
      body = config();
    } catch {
      formError = $t('notifications_string_map');
      return;
    }
    saving = true;
    formError = '';
    try {
      const result = await api.notifications.update(body);
      view = result;
      reset();
      toast.success($t('notifications_saved'));
      await invalidateAll();
    } catch (error) {
      formError = errorMessage(error, $t('failed_to_save_settings'));
      reportUnexpected(error);
    } finally {
      saving = false;
    }
  }
  async function sendTest() {
    if (sendingTest) return;
    sendingTest = true;
    formError = '';
    try {
      await api.notifications.test(currentLocale());
      toast.success($t('notifications_test_sent'));
    } catch (error) {
      formError = errorMessage(error, $t('notifications_test_failed'));
      reportUnexpected(error);
    } finally {
      sendingTest = false;
    }
  }
  async function showPreview() {
    let body: NotificationConfig;
    try {
      body = config();
    } catch {
      formError = $t('notifications_string_map');
      return;
    }
    previewing = true;
    formError = '';
    preview = '';
    try {
      preview = JSON.stringify([await api.notifications.preview(body)], null, 2);
    } catch (error) {
      formError = errorMessage(error, $t('notifications_preview_failed'));
      reportUnexpected(error);
    } finally {
      previewing = false;
    }
  }
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>Alertmanager</Card.Title>
    {#if view.feature_enabled}<Card.Description
        >{$t('notifications_alertmanager_description')}</Card.Description
      >{/if}
  </Card.Header>
  <Card.Content class="space-y-4">
    {#if !view.feature_enabled}
      <p class="text-muted-foreground text-sm">{$t('notifications_feature_disabled')}</p>
    {:else}
      <p class="text-muted-foreground text-sm">{$t('notifications_lifecycle')}</p>
      {#if readonly}<p class="rounded-md border p-3 text-sm">
          {$t('notifications_yaml_readonly')}
        </p>{/if}
      <div class="flex items-center justify-between gap-4">
        <span class="text-sm font-medium">{$t('notifications_alertmanager_enabled')}</span>
        <Button
          variant={enabled ? 'default' : 'outline'}
          size="sm"
          aria-pressed={enabled}
          disabled={readonly || saving}
          onclick={() => (enabled = !enabled)}>{enabled ? $t('enabled') : $t('disabled')}</Button
        >
      </div>
      <div class="grid gap-2">
        <label for="alertmanager-url" class="text-sm font-medium"
          >{$t('notifications_base_url')}</label
        >
        <Input
          id="alertmanager-url"
          type="url"
          placeholder={view.default_url || 'http://alertmanager:9093'}
          bind:value={url}
          disabled={readonly || saving}
        />
      </div>
      {#if view.default_url}<p class="text-muted-foreground text-sm">
          {$t('notifications_default_url', { values: { url: view.default_url } })}
        </p>{/if}
      <p class="text-muted-foreground text-sm">{$t('notifications_identity')}</p>
      <div class="grid gap-4 lg:grid-cols-2">
        <div class="grid gap-2">
          <label for="alertmanager-labels" class="text-sm font-medium"
            >{$t('notifications_labels')}</label
          >
          <textarea
            id="alertmanager-labels"
            class="bg-background min-h-56 w-full rounded-md border p-3 font-mono text-sm"
            spellcheck="false"
            bind:value={labels}
            disabled={readonly || saving}
          ></textarea>
        </div>
        <div class="grid gap-2">
          <label for="alertmanager-annotations" class="text-sm font-medium"
            >{$t('notifications_annotations')}</label
          >
          <textarea
            id="alertmanager-annotations"
            class="bg-background min-h-56 w-full rounded-md border p-3 font-mono text-sm"
            spellcheck="false"
            bind:value={annotations}
            disabled={readonly || saving}
          ></textarea>
        </div>
      </div>
      <details class="rounded-md border p-3">
        <summary class="cursor-pointer text-sm font-medium">{$t('notifications_variables')}</summary
        >
        <p class="text-muted-foreground mt-2 text-sm">{$t('notifications_template_help')}</p>
        <div class="mt-2 flex flex-wrap gap-2">
          {#each view.variables as variable}<code class="bg-muted rounded px-2 py-1 text-xs"
              >{'{{ ' + variable + ' }}'}</code
            >{/each}
        </div>
      </details>
      {#if formError}<p role="alert" class="text-destructive text-sm">{formError}</p>{/if}
      <div class="flex flex-wrap gap-2">
        {#if !readonly}
          <Button onclick={save} disabled={saving || !dirty}
            >{saving ? $t('saving') : $t('save_changes')}</Button
          >
          <Button variant="ghost" onclick={reset} disabled={saving || !dirty}>{$t('reset')}</Button>
        {/if}
        <Button variant="outline" onclick={showPreview} disabled={saving || previewing}
          >{$t('notifications_preview')}</Button
        >
        {#if view.config.alertmanager.enabled}
          <Button variant="outline" onclick={sendTest} disabled={saving || sendingTest || dirty}>
            {sendingTest ? $t('notifications_test_sending') : $t('notifications_test')}
          </Button>
        {/if}
      </div>
      {#if view.config.alertmanager.enabled}
        <p class="text-muted-foreground text-sm">{$t('notifications_test_help')}</p>
      {/if}
      {#if preview}<pre
          class="bg-muted max-h-96 overflow-auto rounded-md p-3 text-xs">{preview}</pre>{/if}
    {/if}
  </Card.Content>
</Card.Root>
