<script lang="ts">
  import { goto, invalidateAll } from '$app/navigation';
  import {
    teams as teamsApi,
    errorMessage,
    type Team,
    type AdminUser,
    type Invite,
    type TeamRole
  } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
  import { authStore } from '$lib/stores/auth.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import * as Card from '$lib/components/ui/card';
  import * as Dialog from '$lib/components/ui/dialog';
  import * as Table from '$lib/components/ui/table';
  import { toast } from '$lib/components/ui/sonner';
  import CopyField from '$lib/components/copy-field.svelte';
  import { formatRelative } from '$lib/format';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import UserPlus from '@lucide/svelte/icons/user-plus';
  import UserMinus from '@lucide/svelte/icons/user-minus';
  import FolderKanban from '@lucide/svelte/icons/folder-kanban';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import PageTitle from '$lib/components/page-title.svelte';
  import RoleHint from '$lib/components/role-hint.svelte';
  import type { PageData } from './$types';

  // Team detail: members (with team roles), assigned projects, and invites.
  // Renaming/deleting the team is instance-manager only; managing membership,
  // team roles, and invites is allowed for an instance manager OR a Team Admin
  // of this team. The backend enforces all of this.
  let { data }: { data: PageData } = $props();

  const team = $derived<Team>(data.team);
  const allUsers = $derived<AdminUser[]>(data.users);

  // Instance-level management (Owner | Manager): create/rename/delete teams and
  // manage any team.
  const canManageInstance = $derived(authStore.canManageInstance);

  // The current user's role within *this* team (if a member).
  const myRole = $derived<TeamRole | null>(
    team.members.find((m) => m.id === authStore.user?.id)?.role ?? null
  );

  // Whether the current user may manage this team's membership, roles, and
  // invites: an instance manager OR a Team Admin of this team.
  const canManageMembers = $derived(canManageInstance || myRole === 'admin');

  // Count of Team Admins — used for last-Team-Admin protection: the only
  // remaining Admin cannot be demoted or removed.
  const adminCount = $derived(team.members.filter((m) => m.role === 'admin').length);

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
      reportUnexpected(err);
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
      reportUnexpected(err);
      deleting = false;
    }
  }

  // --- Add member ---
  let addOpen = $state(false);
  let selectedUserId = $state('');
  let selectedRole = $state<TeamRole>('contributor');
  let adding = $state(false);

  function openAdd() {
    selectedUserId = candidates[0]?.id ?? '';
    selectedRole = 'contributor';
    addOpen = true;
  }

  async function addMember(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedUserId) return;
    adding = true;
    try {
      await teamsApi.addMember(team.id, { user_id: selectedUserId, role: selectedRole });
      toast.success('Member added');
      addOpen = false;
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to add member'));
      reportUnexpected(err);
    } finally {
      adding = false;
    }
  }

  // --- Change member role ---
  // Per-member in-flight flag so the role selector disables while saving and we
  // avoid double-submits.
  let busyMemberId = $state<string | null>(null);

  // Whether demoting `member` away from Admin is blocked by last-Admin
  // protection (the team must keep at least one Admin). Disables the
  // Contributor option for the sole remaining Admin.
  function demoteBlocked(member: { role: TeamRole }): boolean {
    return member.role === 'admin' && adminCount <= 1;
  }

  async function changeRole(userId: string, role: TeamRole) {
    busyMemberId = userId;
    try {
      await teamsApi.setMemberRole(team.id, userId, { role });
      toast.success('Role updated');
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to update role'));
      reportUnexpected(err);
      // Snap the selector back to the server's value on failure.
      await invalidateAll();
    } finally {
      busyMemberId = null;
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
      reportUnexpected(err);
    } finally {
      removingId = null;
    }
  }

  // --- Invites ---
  // Team-scoped invites: a link (optionally emailed) that grants a team role on
  // accept. Manageable by the same callers as membership.
  let invites = $state<Invite[]>([]);
  let invitesLoaded = $state(false);
  let inviteEmail = $state('');
  let inviteRole = $state<TeamRole>('contributor');
  let creatingInvite = $state(false);

  // Pending = not yet accepted.
  const pendingInvites = $derived(invites.filter((i) => !i.accepted_at));

  // Load invites lazily once, only for callers who can manage the team. A
  // forbidden response just yields no invites (the section stays empty).
  $effect(() => {
    if (!canManageMembers || invitesLoaded) return;
    invitesLoaded = true;
    teamsApi
      .invites(team.id)
      .then((list) => {
        invites = list;
      })
      .catch(() => {
        invites = [];
      });
  });

  async function createInvite(event: SubmitEvent) {
    event.preventDefault();
    creatingInvite = true;
    try {
      const email = inviteEmail.trim();
      const invite = await teamsApi.createInvite(team.id, {
        role: inviteRole,
        email: email.length > 0 ? email : undefined
      });
      invites = [invite, ...invites];
      inviteEmail = '';
      toast.success(invite.email_sent ? 'Invite created and emailed' : 'Invite link created');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to create invite'));
      reportUnexpected(err);
    } finally {
      creatingInvite = false;
    }
  }

  async function revokeInvite(invite: Invite) {
    try {
      await teamsApi.revokeInvite(team.id, invite.token);
      invites = invites.filter((i) => i.token !== invite.token);
      toast.success('Invite revoked');
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to revoke invite'));
      reportUnexpected(err);
    }
  }
</script>

<PageTitle title={team.name} />

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
    {#if canManageInstance}
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
      {#if canManageMembers}
        <Button
          variant="secondary"
          size="sm"
          class="gap-1.5"
          onclick={openAdd}
          disabled={candidates.length === 0}
        >
          <UserPlus class="size-4" />
          Add member
        </Button>
      {/if}
    </Card.Header>
    <Card.Content class="space-y-4">
      <div class="bg-muted/40 rounded-lg border p-3">
        <RoleHint scope="team" />
      </div>
      {#if team.members.length === 0}
        <p class="text-muted-foreground py-4 text-sm">No members yet.</p>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Name</Table.Head>
              <Table.Head>Email</Table.Head>
              <Table.Head>Role</Table.Head>
              {#if canManageMembers}
                <Table.Head class="w-12 text-center">Actions</Table.Head>
              {/if}
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each team.members as member (member.id)}
              <Table.Row>
                <Table.Cell class="font-medium">{member.display_name}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{member.email}</Table.Cell>
                <Table.Cell>
                  {#if canManageMembers}
                    <select
                      aria-label={`Team role for ${member.display_name}`}
                      value={member.role}
                      disabled={busyMemberId === member.id}
                      onchange={(e) => changeRole(member.id, e.currentTarget.value as TeamRole)}
                      class="border-input dark:bg-input/30 focus-visible:border-ring focus-visible:ring-ring/50 h-8 rounded-lg border bg-transparent px-2.5 text-sm capitalize transition-colors outline-none focus-visible:ring-3 disabled:opacity-50"
                    >
                      <option value="admin">Admin</option>
                      <option value="contributor" disabled={demoteBlocked(member)}
                        >Contributor</option
                      >
                    </select>
                  {:else}
                    <Badge
                      variant="outline"
                      class={member.role === 'admin' ? 'text-primary capitalize' : 'capitalize'}
                    >
                      {member.role}
                    </Badge>
                  {/if}
                </Table.Cell>
                {#if canManageMembers}
                  <Table.Cell class="w-12 text-center">
                    <Button
                      variant="ghost"
                      size="icon"
                      class="text-destructive hover:text-destructive size-8"
                      title="Remove member"
                      disabled={removingId === member.id ||
                        (member.role === 'admin' && adminCount <= 1)}
                      onclick={() => removeMember(member.id)}
                    >
                      <UserMinus class="size-4" />
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

  {#if canManageMembers}
    <Card.Root>
      <Card.Header>
        <Card.Title>Invites</Card.Title>
        <Card.Description>
          Generate an invite link to add someone to this team. The link works even without email; if
          SMTP is configured and you provide an address, it is also emailed.
        </Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        <form class="flex flex-wrap items-end gap-3" onsubmit={createInvite}>
          <div class="min-w-48 flex-1 space-y-2">
            <label for="invite-email" class="text-sm font-medium">Email (optional)</label>
            <Input
              id="invite-email"
              type="email"
              placeholder="teammate@example.com"
              bind:value={inviteEmail}
            />
          </div>
          <div class="space-y-2">
            <label for="invite-role" class="text-sm font-medium">Role</label>
            <select
              id="invite-role"
              bind:value={inviteRole}
              class="border-input dark:bg-input/30 focus-visible:border-ring focus-visible:ring-ring/50 h-8 rounded-lg border bg-transparent px-2.5 text-sm transition-colors outline-none focus-visible:ring-3"
            >
              <option value="contributor">Contributor</option>
              <option value="admin">Admin</option>
            </select>
          </div>
          <Button type="submit" disabled={creatingInvite}>
            <UserPlus class="size-4" />
            {creatingInvite ? 'Creating…' : 'Create invite'}
          </Button>
        </form>

        <RoleHint scope="team" />

        {#if pendingInvites.length > 0}
          <div class="space-y-3">
            {#each pendingInvites as invite (invite.token)}
              <div class="space-y-2 rounded-md border p-3">
                <div class="flex items-center justify-between gap-2">
                  <div class="flex items-center gap-2 text-sm">
                    <Badge
                      variant="outline"
                      class={invite.role === 'admin' ? 'text-primary capitalize' : 'capitalize'}
                    >
                      {invite.role}
                    </Badge>
                    {#if invite.email}
                      <span>{invite.email}</span>
                    {:else}
                      <span class="text-muted-foreground">Anyone with the link</span>
                    {/if}
                    <span class="text-muted-foreground text-xs"
                      >· expires {formatRelative(invite.expires_at)}</span
                    >
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="size-8"
                    onclick={() => revokeInvite(invite)}
                    aria-label="Revoke invite"
                  >
                    <Trash2 class="text-destructive size-4" />
                  </Button>
                </div>
                <CopyField value={invite.link} label="invite link" />
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-muted-foreground text-sm">No pending invites.</p>
        {/if}
      </Card.Content>
    </Card.Root>
  {/if}

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

<!-- Add member dialog -->
<Dialog.Root bind:open={addOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Add member</Dialog.Title>
      <Dialog.Description>Select a user and a role to add to this team.</Dialog.Description>
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
      <div class="space-y-2">
        <label for="add-member-role" class="text-sm font-medium">Role</label>
        <select
          id="add-member-role"
          bind:value={selectedRole}
          class="border-input focus-visible:border-ring focus-visible:ring-ring/50 h-8 w-full rounded-lg border bg-transparent px-2.5 py-1 text-sm outline-none focus-visible:ring-3"
        >
          <option value="contributor">Contributor</option>
          <option value="admin">Admin</option>
        </select>
        <RoleHint scope="team" />
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
