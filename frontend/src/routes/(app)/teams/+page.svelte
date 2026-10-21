<script lang="ts">
  import { goto } from '$app/navigation';
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

  const canManageInstance = $derived(authStore.canManageInstance);

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
      toast.success(`Team “${team.name}” created`);
      createOpen = false;
      newName = '';
      await goto(`/teams/${team.id}`);
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to create team'));
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

<PageTitle title="Teams" />

<div class="space-y-6">
  <PageHeader title="Teams">
    {#snippet description()}
      Groups of users that grant access to assigned projects.
    {/snippet}
    {#snippet actions()}
      {#if canManageInstance}
        <Button onclick={openCreate} class="gap-1.5">
          <Plus class="size-4" />
          New team
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
        <p class="font-medium">No teams yet</p>
        <p class="text-muted-foreground max-w-xs text-sm">
          Teams group users and grant them access to the team’s projects.
        </p>
        {#if canManageInstance}
          <Button variant="outline" size="sm" onclick={openCreate} class="mt-3 gap-1.5">
            <Plus class="size-4" />
            Create your first team
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
      <Dialog.Title>New team</Dialog.Title>
      <Dialog.Description>Give the team a name. You can add members afterwards.</Dialog.Description>
    </Dialog.Header>
    <form onsubmit={createTeam} class="space-y-4">
      <div class="space-y-2">
        <label for="team-name" class="text-sm font-medium">Name</label>
        <Input id="team-name" bind:value={newName} placeholder="e.g. Backend" required />
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
        <Button type="submit" disabled={creating || newName.trim().length === 0}>
          {creating ? 'Creating…' : 'Create team'}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
