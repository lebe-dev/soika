<script lang="ts">
  import { goto } from '$app/navigation';
  import { teams as teamsApi, errorMessage, type TeamSummary } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import * as Card from '$lib/components/ui/card';
  import * as Dialog from '$lib/components/ui/dialog';
  import * as Table from '$lib/components/ui/table';
  import { toast } from '$lib/components/ui/sonner';
  import Plus from '@lucide/svelte/icons/plus';
  import Users from '@lucide/svelte/icons/users';
  import type { PageData } from './$types';

  // Teams — team list. Any authenticated user can browse teams;
  // creating a team is restricted to the instance admin.
  let { data }: { data: PageData } = $props();

  const isAdmin = $derived(authStore.isAdmin);

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

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Teams</h1>
      <p class="text-muted-foreground text-sm">
        Groups of users that grant access to assigned projects.
      </p>
    </div>
    {#if isAdmin}
      <Button onclick={openCreate} class="gap-2">
        <Plus class="size-4" />
        New team
      </Button>
    {/if}
  </div>

  <Card.Root>
    <Card.Content class="pt-6">
      {#if list.length === 0}
        <div class="flex flex-col items-center gap-3 py-10 text-center">
          <Users class="text-muted-foreground size-8" />
          <p class="text-muted-foreground text-sm">No teams yet.</p>
          {#if isAdmin}
            <Button variant="outline" onclick={openCreate} class="gap-2">
              <Plus class="size-4" />
              Create your first team
            </Button>
          {/if}
        </div>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Name</Table.Head>
              <Table.Head class="text-right">Members</Table.Head>
              <Table.Head class="text-right">Projects</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each list as team (team.id)}
              <Table.Row class="cursor-pointer" onclick={() => goto(`/teams/${team.id}`)}>
                <Table.Cell class="font-medium">
                  <a
                    href={`/teams/${team.id}`}
                    class="underline-offset-4 hover:underline"
                    onclick={(e) => e.stopPropagation()}
                  >
                    {team.name}
                  </a>
                </Table.Cell>
                <Table.Cell class="text-right">
                  <Badge variant="secondary">{team.member_count}</Badge>
                </Table.Cell>
                <Table.Cell class="text-right">
                  <Badge variant="secondary">{team.project_count}</Badge>
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </Card.Content>
  </Card.Root>
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
