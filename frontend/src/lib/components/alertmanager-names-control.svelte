<script lang="ts">
  import { untrack } from 'svelte';
  import { invalidateAll } from '$app/navigation';
  import { notifications, errorMessage, type AlertmanagerNames } from '$lib/api';
  import { t } from '$lib/i18n';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import { toast } from '$lib/components/ui/sonner';

  let { names, readonly = false }: { names: AlertmanagerNames; readonly?: boolean } = $props();
  let draft = $state(untrack(() => ({ ...names })));
  let saving = $state(false);
  let error = $state('');
  const fields = ['critical', 'warning', 'info', 'project_test'] as const;
  const dirty = $derived(fields.some((key) => draft[key] !== names[key]));
  const valid = $derived(fields.every((key) => draft[key].trim()));

  async function save(event: SubmitEvent) {
    event.preventDefault();
    saving = true;
    error = '';
    try {
      names = await notifications.setNames(draft);
      draft = { ...names };
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
    <Card.Title>{$t('alertmanager_names_title')}</Card.Title>
    <Card.Description>{$t('alertmanager_names_help')}</Card.Description>
  </Card.Header>
  <Card.Content>
    <form onsubmit={save} class="space-y-4" data-managed-readonly={readonly || undefined}>
      <div class="grid gap-4 sm:grid-cols-2">
        {#each fields as key}
          <div class="grid gap-2">
            <label for={'alertmanager-name-' + key} class="text-sm font-medium">
              {$t(`alertmanager_name_${key}`)}
            </label>
            <Input
              id={'alertmanager-name-' + key}
              class="font-mono"
              bind:value={draft[key]}
              disabled={readonly || saving}
              required
            />
          </div>
        {/each}
      </div>
      {#if error}<p role="alert" class="text-destructive text-sm">{error}</p>{/if}
      {#if !readonly}
        <Button type="submit" disabled={saving || !dirty || !valid}>
          {saving ? $t('saving') : $t('save_changes')}
        </Button>
      {/if}
    </form>
  </Card.Content>
</Card.Root>
