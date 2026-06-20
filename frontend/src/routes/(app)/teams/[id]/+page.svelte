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
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { toast } from '$lib/components/ui/sonner';
  import CopyField from '$lib/components/copy-field.svelte';
  import { formatRelative } from '$lib/format';
  import { cn } from '$lib/utils';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import UserPlus from '@lucide/svelte/icons/user-plus';
  import UserMinus from '@lucide/svelte/icons/user-minus';
  import FolderKanban from '@lucide/svelte/icons/folder-kanban';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';
  import Link2 from '@lucide/svelte/icons/link-2';
  import Info from '@lucide/svelte/icons/info';
  import PageTitle from '$lib/components/page-title.svelte';
  import RoleHint from '$lib/components/role-hint.svelte';
  import type { PageData } from './$types';

  // --- Avatar helpers -------------------------------------------------------
  // Initials (first + last word) for the member/team chip, and a deterministic
  // soft tint per id so the roster reads at a glance without being loud.
  function initials(name: string): string {
    const parts = name.trim().split(/\s+/).filter(Boolean);
    if (parts.length === 0) return '?';
    if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
    return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  }

  const AVATAR_TINTS = [
    'bg-primary/10 text-primary',
    'bg-pink-500/10 text-pink-600 dark:text-pink-400',
    'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400',
    'bg-amber-500/10 text-amber-600 dark:text-amber-500',
    'bg-violet-500/10 text-violet-600 dark:text-violet-400',
    'bg-cyan-500/10 text-cyan-600 dark:text-cyan-400'
  ];

  function avatarTint(id: string): string {
    let h = 0;
    for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
    return AVATAR_TINTS[h % AVATAR_TINTS.length];
  }

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
  let inviteOpen = $state(false);
  let inviteEmail = $state('');
  let inviteRole = $state<TeamRole>('contributor');
  let creatingInvite = $state(false);

  function openInvite() {
    inviteEmail = '';
    inviteRole = 'contributor';
    inviteOpen = true;
  }

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
      inviteOpen = false;
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

  <!-- Identity: team chip + soft stat pills. The pills carry the at-a-glance
       summary so the sections below can stay quiet. -->
  <div class="flex items-start justify-between gap-4">
    <div class="flex items-start gap-3.5">
      <span
        class="bg-primary/10 text-primary grid size-12 shrink-0 place-items-center rounded-xl text-base font-semibold"
        aria-hidden="true"
      >
        {initials(team.name)}
      </span>
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{team.name}</h1>
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <span class="bg-muted text-muted-foreground rounded-full px-3 py-1 text-xs">
            <span class="text-foreground font-medium">{team.members.length}</span>
            {team.members.length === 1 ? 'member' : 'members'}
          </span>
          <span class="bg-muted text-muted-foreground rounded-full px-3 py-1 text-xs">
            <span class="text-foreground font-medium">{team.projects.length}</span>
            {team.projects.length === 1 ? 'project' : 'projects'}
          </span>
          {#if myRole}
            <span class="bg-muted text-muted-foreground rounded-full px-3 py-1 text-xs">
              you’re <span class="text-foreground font-medium capitalize">{myRole}</span>
            </span>
          {/if}
        </div>
      </div>
    </div>
    {#if canManageInstance}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          <Button variant="outline" size="icon" aria-label="Team actions">
            <EllipsisVertical class="size-4" />
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="w-40">
          <DropdownMenu.Item onclick={openRename}>
            <Pencil class="size-4" />
            Rename
          </DropdownMenu.Item>
          <DropdownMenu.Item variant="destructive" onclick={() => (deleteOpen = true)}>
            <Trash2 class="size-4" />
            Delete
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/if}
  </div>

  <!-- Members: the primary surface. Soft top accent, roster with avatars.
       Invites live here as a secondary action (dialog), not their own block. -->
  <Card.Root class="border-t-primary border-t-[3px] shadow-sm">
    <Card.Header class="flex-row items-start justify-between space-y-0">
      <div class="space-y-1">
        <div class="flex items-center gap-1.5">
          <Card.Title>Members</Card.Title>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              <button
                type="button"
                class="text-muted-foreground hover:text-foreground transition-colors"
                aria-label="What can each role do?"
              >
                <Info class="size-4" />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="start" class="w-80 p-3">
              <RoleHint scope="team" />
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </div>
        <Card.Description>Who’s in this team and what role they have.</Card.Description>
      </div>
      {#if canManageMembers}
        <div class="flex shrink-0 items-center gap-2">
          <Button variant="outline" size="sm" class="gap-1.5" onclick={openInvite}>
            <Link2 class="size-4" />
            Invite by link
          </Button>
          <Button
            variant="default"
            size="sm"
            class="gap-1.5"
            onclick={openAdd}
            disabled={candidates.length === 0}
          >
            <UserPlus class="size-4" />
            Add member
          </Button>
        </div>
      {/if}
    </Card.Header>
    <Card.Content class="space-y-4">
      {#if canManageMembers && pendingInvites.length > 0}
        <div class="bg-muted/30 space-y-2 rounded-lg border p-3">
          <p class="text-muted-foreground text-xs font-medium tracking-wide uppercase">
            Pending invites
          </p>
          {#each pendingInvites as invite (invite.token)}
            <div class="space-y-2">
              <div class="flex items-center justify-between gap-2">
                <div class="flex min-w-0 items-center gap-2 text-sm">
                  <Badge
                    variant="outline"
                    class={invite.role === 'admin' ? 'text-primary capitalize' : 'capitalize'}
                  >
                    {invite.role}
                  </Badge>
                  {#if invite.email}
                    <span class="truncate">{invite.email}</span>
                  {:else}
                    <span class="text-muted-foreground">Anyone with the link</span>
                  {/if}
                  <span class="text-muted-foreground shrink-0 text-xs"
                    >· expires {formatRelative(invite.expires_at)}</span
                  >
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-8 shrink-0"
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
      {/if}

      {#if team.members.length === 0}
        <div class="flex flex-col items-center gap-1.5 py-8 text-center">
          <span class="bg-muted text-primary grid size-11 place-items-center rounded-xl">
            <UserPlus class="size-5" />
          </span>
          <p class="font-medium">No members yet</p>
          <p class="text-muted-foreground max-w-xs text-sm">
            Add a teammate or share an invite link to give access to this team’s projects.
          </p>
          {#if canManageMembers}
            <div class="mt-3 flex items-center gap-2">
              <Button variant="outline" size="sm" class="gap-1.5" onclick={openInvite}>
                <Link2 class="size-4" />
                Invite by link
              </Button>
              <Button
                size="sm"
                class="gap-1.5"
                onclick={openAdd}
                disabled={candidates.length === 0}
              >
                <UserPlus class="size-4" />
                Add member
              </Button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="divide-border/60 divide-y">
          {#each team.members as member (member.id)}
            <div class="flex items-center gap-3 py-3 first:pt-0 last:pb-0">
              <span
                class={cn(
                  'grid size-9 shrink-0 place-items-center rounded-full text-xs font-semibold',
                  avatarTint(member.id)
                )}
                aria-hidden="true"
              >
                {initials(member.display_name)}
              </span>
              <div class="min-w-0 flex-1">
                <p class="truncate font-medium">{member.display_name}</p>
                <p class="text-muted-foreground truncate text-sm">{member.email}</p>
              </div>
              {#if canManageMembers}
                <select
                  aria-label={`Team role for ${member.display_name}`}
                  value={member.role}
                  disabled={busyMemberId === member.id}
                  onchange={(e) => changeRole(member.id, e.currentTarget.value as TeamRole)}
                  class="border-input dark:bg-input/30 focus-visible:border-primary focus-visible:ring-primary/30 h-8 shrink-0 rounded-lg border bg-transparent px-2.5 text-sm capitalize transition-colors outline-none focus-visible:ring-1 disabled:opacity-50"
                >
                  <option value="admin">Admin</option>
                  <option value="contributor" disabled={demoteBlocked(member)}>Contributor</option>
                </select>
                <Button
                  variant="ghost"
                  size="icon"
                  class="text-destructive hover:text-destructive size-8 shrink-0"
                  title="Remove member"
                  disabled={removingId === member.id ||
                    (member.role === 'admin' && adminCount <= 1)}
                  onclick={() => removeMember(member.id)}
                >
                  <UserMinus class="size-4" />
                </Button>
              {:else}
                <Badge
                  variant="outline"
                  class={member.role === 'admin'
                    ? 'text-primary shrink-0 capitalize'
                    : 'shrink-0 capitalize'}
                >
                  {member.role}
                </Badge>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </Card.Content>
  </Card.Root>

  <!-- Projects: secondary reference — what this team can see. Compact chips,
       not a full table; assignment happens on the project itself. -->
  <Card.Root>
    <Card.Header>
      <Card.Title class="text-base">Projects</Card.Title>
      <Card.Description>Projects this team can access.</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if team.projects.length === 0}
        <div class="flex flex-col items-center gap-2 py-6 text-center">
          <FolderKanban class="text-muted-foreground size-7" />
          <p class="text-muted-foreground text-sm">No projects assigned to this team.</p>
        </div>
      {:else}
        <div class="flex flex-wrap gap-2">
          {#each team.projects as project (project.id)}
            <a
              href={`/projects/${project.id}`}
              class="hover:border-primary/40 hover:bg-muted/50 group inline-flex items-center gap-2 rounded-lg border px-3 py-2 text-sm transition-colors"
            >
              <FolderKanban class="text-primary size-4 shrink-0" />
              <span class="font-medium">{project.name}</span>
              <span class="text-muted-foreground font-mono text-xs">{project.slug}</span>
              <ChevronRight
                class="text-muted-foreground/60 group-hover:text-foreground size-3.5 transition-colors"
              />
            </a>
          {/each}
        </div>
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

<!-- Invite by link dialog -->
<Dialog.Root bind:open={inviteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Invite by link</Dialog.Title>
      <Dialog.Description>
        Generate a link to add someone to this team. The link works even without email; if SMTP is
        configured and you provide an address, it is also emailed.
      </Dialog.Description>
    </Dialog.Header>
    <form onsubmit={createInvite} class="space-y-4">
      <div class="space-y-2">
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
          class="border-input focus-visible:border-primary focus-visible:ring-primary/30 h-8 w-full rounded-lg border bg-transparent px-2.5 py-1 text-sm outline-none focus-visible:ring-1"
        >
          <option value="contributor">Contributor</option>
          <option value="admin">Admin</option>
        </select>
        <RoleHint scope="team" />
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (inviteOpen = false)}>Cancel</Button>
        <Button type="submit" disabled={creatingInvite}>
          {creatingInvite ? 'Creating…' : 'Create invite'}
        </Button>
      </Dialog.Footer>
    </form>
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
            class="border-input focus-visible:border-primary focus-visible:ring-primary/30 h-8 w-full rounded-lg border bg-transparent px-2.5 py-1 text-sm outline-none focus-visible:ring-1"
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
          class="border-input focus-visible:border-primary focus-visible:ring-primary/30 h-8 w-full rounded-lg border bg-transparent px-2.5 py-1 text-sm outline-none focus-visible:ring-1"
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
