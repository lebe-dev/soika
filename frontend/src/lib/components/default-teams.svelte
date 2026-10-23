<script lang="ts">
  import { untrack } from 'svelte';
  import { invalidateAll } from '$app/navigation';
  import { api, errorMessage, type TeamSummary } from '$lib/api';
  import { t } from '$lib/i18n';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { toast } from '$lib/components/ui/sonner';

  let {
    teams,
    defaultTeamIds
  }: { teams: Pick<TeamSummary, 'id' | 'name'>[]; defaultTeamIds: string[] } = $props();
  let saved = $state<string[]>(untrack(() => [...defaultTeamIds]));
  let selected = $state<string[]>(untrack(() => [...defaultTeamIds]));
  let saving = $state(false);

  // Refresh the form when route data changes, including deletion of a default team.
  $effect(() => {
    saved = [...defaultTeamIds];
    selected = [...defaultTeamIds];
  });
  const dirty = $derived(
    selected.length !== saved.length || selected.some((id) => !saved.includes(id))
  );

  async function save() {
    if (!dirty || saving) return;
    saving = true;
    try {
      const result = await api.settings.update({ default_team_ids: [...selected] });
      saved = [...result.default_team_ids];
      selected = [...result.default_team_ids];
      toast.success($t('service_settings_saved'));
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_save_settings')));
      reportUnexpected(err);
    } finally {
      saving = false;
    }
  }
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>{$t('default_teams_title')}</Card.Title>
    <Card.Description>{$t('default_teams_description')}</Card.Description>
  </Card.Header>
  <Card.Content class="space-y-4">
    {#if teams.length === 0}
      <p class="text-muted-foreground text-sm">{$t('default_teams_empty')}</p>
    {:else}
      <fieldset disabled={saving} class="grid gap-2 sm:grid-cols-2">
        <legend class="sr-only">{$t('default_teams_title')}</legend>
        {#each teams as team (team.id)}
          <label class="flex cursor-pointer items-center gap-3 rounded-lg border p-3 text-sm">
            <input
              type="checkbox"
              value={team.id}
              bind:group={selected}
              class="accent-primary size-4 shrink-0"
            />
            <span class="min-w-0 break-words">{team.name}</span>
          </label>
        {/each}
      </fieldset>
    {/if}
    <div class="flex items-center gap-2">
      <Button onclick={save} disabled={!dirty || saving}>
        {saving ? $t('saving') : $t('save_changes')}
      </Button>
      <Button variant="ghost" onclick={() => (selected = [...saved])} disabled={!dirty || saving}>
        {$t('reset')}
      </Button>
    </div>
  </Card.Content>
</Card.Root>
