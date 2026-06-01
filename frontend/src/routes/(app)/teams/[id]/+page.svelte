<script lang="ts">
  import { goto, invalidateAll } from '$app/navigation';
  import { teams as teamsApi, errorMessage, type Team, type AdminUser } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import * as Card from '$lib/components/ui/card';
  import * as Dialog from '$lib/components/ui/dialog';
  import * as Table from '$lib/components/ui/table';
  import { toast } from '$lib/components/ui/sonner';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import UserPlus from '@lucide/svelte/icons/user-plus';
  import UserMinus from '@lucide/svelte/icons/user-minus';
  import FolderKanban from '@lucide/svelte/icons/folder-kanban';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import type { PageData } from './$types';

  // Team detail members + assigned projects, with admin-only rename,
  // delete, and member management.
  let { data }: { data: PageData } = $props();

  const team = $derived<Team>(data.team);
  const allUsers = $derived<AdminUser[]>(data.users);
  const isAdmin = $derived(authStore.isAdmin);

  // Users who are not already members — candidates for the add-member picker.
  const memberIds = $derived(new Set(team.members.map((m) => m.id)));
  const candidates = $derived(allUsers.filter((u) => !memberIds.has(u.id)));

  // --- Rename ---
  let renameOpen = $state(false);
  let renameName = $state('');
  let renaming = $state(false);

  function openRename() {
    renameName = team.name;
    renameOpen = true;
  }

  async function renameTeam(event: SubmitEvent) {
    event.preventDefault();
    const name = renameName.trim();
    if (!name) return;
    renaming = true;
    try {
      await teamsApi.update(team.id, { name });
      toast.success('Team renamed');
      renameOpen = false;
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to rename team'));
    } finally {
      renaming = false;
    }
  }

  // --- Delete ---
  let deleteOpen = $state(false);
  let deleting = $state(false);

  async function deleteTeam() {
    deleting = true;
    try {
      await teamsApi.remove(team.id);
      toast.success('Team deleted');
      deleteOpen = false;
      await goto('/teams');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to delete team'));
      deleting = false;
    }
  }

  // --- Add member ---
  let addOpen = $state(false);
  let selectedUserId = $state('');
  let adding = $state(false);

  function openAdd() {
    selectedUserId = candidates[0]?.id ?? '';
    addOpen = true;
  }

  async function addMember(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedUserId) return;
    adding = true;
    try {
      await teamsApi.addMember(team.id, { user_id: selectedUserId });
      toast.success('Member added');
      addOpen = false;
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to add member'));
    } finally {
      adding = false;
    }
  }

  // --- Remove member ---
  let removingId = $state<string | null>(null);

  async function removeMember(userId: string) {
    removingId = userId;
    try {
      await teamsApi.removeMember(team.id, userId);
      toast.success('Member removed');
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to remove member'));
    } finally {
      removingId = null;
    }
  }
</script>

<div class="space-y-6">
  <div>
    <a
      href="/teams"
      class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1 text-sm"
    >
      <ChevronLeft class="size-4" />
      Teams
    </a>
  </div>

  <div class="flex items-start justify-between gap-4">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">{team.name}</h1>
      <p class="text-muted-foreground text-sm">
        {team.members.length}
        {team.members.length === 1 ? 'member' : 'members'} ·
        {team.projects.length}
        {team.projects.length === 1 ? 'project' : 'projects'}
      </p>
    </div>
    {#if isAdmin}
      <div class="flex items-center gap-2">
        <Button variant="outline" size="sm" class="gap-1.5" onclick={openRename}>
          <Pencil class="size-4" />
          Rename
        </Button>
        <Button variant="outline" size="sm" class="gap-1.5" onclick={() => (deleteOpen = true)}>
          <Trash2 class="size-4" />
          Delete
        </Button>
      </div>
    {/if}
  </div>

  <Card.Root>
    <Card.Header class="flex-row items-center justify-between space-y-0">
      <div>
        <Card.Title>Members</Card.Title>
        <Card.Description>Users in this team can access its projects.</Card.Description>
      </div>
      {#if isAdmin}
        <Button size="sm" class="gap-1.5" onclick={openAdd} disabled={candidates.length === 0}>
          <UserPlus class="size-4" />
          Add member
        </Button>
      {/if}
    </Card.Header>
    <Card.Content>
      {#if team.members.length === 0}
        <p class="text-muted-foreground py-4 text-sm">No members yet.</p>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Name</Table.Head>
              <Table.Head>Email</Table.Head>
              {#if isAdmin}
                <Table.Head class="text-right">Actions</Table.Head>
              {/if}
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each team.members as member (member.id)}
              <Table.Row>
                <Table.Cell class="font-medium">{member.display_name}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{member.email}</Table.Cell>
                {#if isAdmin}
                  <Table.Cell class="text-right">
                    <Button
                      variant="ghost"
                      size="sm"
                      class="text-destructive hover:text-destructive gap-1.5"
                      disabled={removingId === member.id}
                      onclick={() => removeMember(member.id)}
                    >
                      <UserMinus class="size-4" />
                      {removingId === member.id ? 'Removing…' : 'Remove'}
                    </Button>
                  </Table.Cell>
                {/if}
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </Card.Content>
  </Card.Root>

  <Card.Root>
    <Card.Header>
      <Card.Title>Projects</Card.Title>
      <Card.Description>Projects assigned to this team.</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if team.projects.length === 0}
        <div class="flex flex-col items-center gap-2 py-6 text-center">
          <FolderKanban class="text-muted-foreground size-7" />
          <p class="text-muted-foreground text-sm">No projects assigned to this team.</p>
        </div>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Name</Table.Head>
              <Table.Head>Slug</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each team.projects as project (project.id)}
              <Table.Row class="cursor-pointer" onclick={() => goto(`/projects/${project.id}`)}>
                <Table.Cell class="font-medium">
                  <a
                    href={`/projects/${project.id}`}
                    class="underline-offset-4 hover:underline"
                    onclick={(e) => e.stopPropagation()}
                  >
                    {project.name}
                  </a>
                </Table.Cell>
                <Table.Cell>
                  <Badge variant="outline">{project.slug}</Badge>
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </Card.Content>
  </Card.Root>
</div>

<!-- Rename dialog (admin) -->
<Dialog.Root bind:open={renameOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Rename team</Dialog.Title>
      <Dialog.Description>Update the display name for this team.</Dialog.Description>
    </Dialog.Header>
    <form onsubmit={renameTeam} class="space-y-4">
      <div class="space-y-2">
        <label for="rename-team" class="text-sm font-medium">Name</label>
        <Input id="rename-team" bind:value={renameName} required />
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (renameOpen = false)}>Cancel</Button>
        <Button type="submit" disabled={renaming || renameName.trim().length === 0}>
          {renaming ? 'Saving…' : 'Save'}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete confirmation (admin) -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete team</Dialog.Title>
      <Dialog.Description>
        Delete “{team.name}”? Members lose access to its projects. This cannot be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button type="button" variant="outline" onclick={() => (deleteOpen = false)}>Cancel</Button>
      <Button variant="destructive" disabled={deleting} onclick={deleteTeam}>
        {deleting ? 'Deleting…' : 'Delete team'}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Add member dialog (admin) -->
<Dialog.Root bind:open={addOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Add member</Dialog.Title>
      <Dialog.Description>Select a user to add to this team.</Dialog.Description>
    </Dialog.Header>
    <form onsubmit={addMember} class="space-y-4">
      <div class="space-y-2">
        <label for="add-member" class="text-sm font-medium">User</label>
        {#if candidates.length === 0}
          <p class="text-muted-foreground text-sm">All users are already members.</p>
        {:else}
          <select
            id="add-member"
            bind:value={selectedUserId}
            class="border-input focus-visible:border-ring focus-visible:ring-ring/50 h-8 w-full rounded-lg border bg-transparent px-2.5 py-1 text-sm outline-none focus-visible:ring-3"
          >
            {#each candidates as user (user.id)}
              <option value={user.id}>{user.display_name} ({user.email})</option>
            {/each}
          </select>
        {/if}
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (addOpen = false)}>Cancel</Button>
        <Button type="submit" disabled={adding || !selectedUserId}>
          {adding ? 'Adding…' : 'Add member'}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
