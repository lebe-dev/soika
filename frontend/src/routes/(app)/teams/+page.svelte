<script lang="ts">
  import { t } from '$lib/i18n';
  import { goto, invalidateAll } from '$app/navigation';
  import { teams as teamsApi, errorMessage, type TeamSummary } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Card from '$lib/components/ui/card';
  import * as Dialog from '$lib/components/ui/dialog';
  import { toast } from '$lib/components/ui/sonner';
  import Plus from '@lucide/svelte/icons/plus';
  import Users from '@lucide/svelte/icons/users';
  import PageTitle from '$lib/components/page-title.svelte';
  import PageHeader from '$lib/components/page-header.svelte';
  import TeamList from '$lib/components/team-list.svelte';
  import type { PageData } from './$types';

  // Teams — team list. Any authenticated user can browse their teams;
  // creating a team is restricted to instance managers (Owner | Manager).
  let { data }: { data: PageData } = $props();

  const teamsManaged = $derived(data.telemetry?.team_management_source === 'yaml');
  const canManageInstance = $derived(authStore.canManageInstance && !teamsManaged);

  let createOpen = $state(false);
  let newName = $state('');
  let creating = $state(false);

  async function createTeam(event: SubmitEvent) {
    event.preventDefault();
    const name = newName.trim();
    if (!name) return;
    creating = true;
    try {
      const team = await teamsApi.create({ name });
      toast.success($t('team_created', { values: { name: team.name } }));
      createOpen = false;
      newName = '';
      // The dashboard reads its team list from the root auth/config load. Refresh
      // it after creation so returning there doesn't show the stale empty state.
      await invalidateAll();
      await goto(`/teams/${team.id}`);
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_create_team')));
      reportUnexpected(err);
    } finally {
      creating = false;
    }
  }

  function openCreate() {
    newName = '';
    createOpen = true;
  }

  const list = $derived<TeamSummary[]>(data.teams);
</script>

<PageTitle title={$t('teams')} />

<div class="space-y-6">
  <PageHeader title={$t('teams')}>
    {#snippet description()}
      {$t('groups_of_users_that_grant_access_to_assigned_projects')}
    {/snippet}
    {#snippet actions()}
      {#if canManageInstance}
        <Button onclick={openCreate} class="gap-1.5">
          <Plus class="size-4" />
          {$t('new_team')}
        </Button>
      {/if}
    {/snippet}
  </PageHeader>

  {#if list.length === 0}
    <Card.Root class="shadow-sm">
      <Card.Content class="flex flex-col items-center gap-1.5 py-10 text-center">
        <span class="bg-muted text-primary grid size-11 place-items-center rounded-xl">
          <Users class="size-5" />
        </span>
        <p class="font-medium">{$t('no_teams_yet')}</p>
        <p class="text-muted-foreground max-w-xs text-sm">
          {$t('teams_group_users_and_grant_them_access_to_the_team_s_projects')}
        </p>
        {#if canManageInstance}
          <Button variant="outline" size="sm" onclick={openCreate} class="mt-3 gap-1.5">
            <Plus class="size-4" />
            {$t('create_your_first_team')}
          </Button>
        {/if}
      </Card.Content>
    </Card.Root>
  {:else}
    <TeamList teams={list} />
  {/if}
</div>

<Dialog.Root bind:open={createOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{$t('new_team')}</Dialog.Title>
      <Dialog.Description
        >{$t('give_the_team_a_name_you_can_add_members_afterwards')}</Dialog.Description
      >
    </Dialog.Header>
    <form onsubmit={createTeam} class="space-y-4">
      <div class="space-y-2">
        <label for="team-name" class="text-sm font-medium">{$t('name')}</label>
        <Input id="team-name" bind:value={newName} placeholder={$t('e_g_backend')} required />
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (createOpen = false)}
          >{$t('cancel')}</Button
        >
        <Button type="submit" disabled={creating || newName.trim().length === 0}>
          {creating ? $t('creating') : $t('create_team')}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
