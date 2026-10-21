<script lang="ts">
  import { t, locale } from '$lib/i18n';
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
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import UserPlus from '@lucide/svelte/icons/user-plus';
  import UserMinus from '@lucide/svelte/icons/user-minus';
  import FolderKanban from '@lucide/svelte/icons/folder-kanban';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';
  import Link2 from '@lucide/svelte/icons/link-2';
  import Info from '@lucide/svelte/icons/info';
  import PageTitle from '$lib/components/page-title.svelte';
  import RoleHint from '$lib/components/role-hint.svelte';
  import Avatar from '$lib/components/avatar.svelte';
  import EntityHeader from '$lib/components/entity-header.svelte';
  import StatPill from '$lib/components/stat-pill.svelte';
  import { NativeSelect } from '$lib/components/ui/native-select';
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
      toast.success($t('team_renamed'));
      renameOpen = false;
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_rename_team')));
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
      toast.success($t('team_deleted'));
      deleteOpen = false;
      await goto('/teams');
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_delete_team')));
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
      toast.success($t('member_added'));
      addOpen = false;
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_add_member')));
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
      toast.success($t('role_updated'));
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_update_role')));
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
      toast.success($t('member_removed'));
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_remove_member')));
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
      toast.success(
        invite.email_sent ? $t('invite_created_and_emailed') : $t('invite_link_created')
      );
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_create_invite')));
      reportUnexpected(err);
    } finally {
      creatingInvite = false;
    }
  }

  async function revokeInvite(invite: Invite) {
    try {
      await teamsApi.revokeInvite(team.id, invite.token);
      invites = invites.filter((i) => i.token !== invite.token);
      toast.success($t('invite_revoked'));
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_revoke_invite')));
      reportUnexpected(err);
    }
  }
</script>

<PageTitle title={team.name} />

<div class="space-y-6">
  <!-- Identity: team chip + soft stat pills. The pills carry the at-a-glance
       summary so the sections below can stay quiet. -->
  <EntityHeader title={team.name} avatarId={team.id} back={{ href: '/teams', label: $t('teams') }}>
    {#snippet pills()}
      <StatPill>
        {$t('member_count', { values: { count: team.members.length } })}
      </StatPill>
      <StatPill>
        {$t('project_count', { values: { count: team.projects.length } })}
      </StatPill>
      {#if myRole}
        <StatPill>{$t('you_re')} <strong class="capitalize">{$t(myRole)}</strong></StatPill>
      {/if}
    {/snippet}
    {#snippet actions()}
      {#if canManageInstance}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>
            <Button variant="outline" size="icon" aria-label={$t('team_actions')}>
              <EllipsisVertical class="size-4" />
            </Button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Content align="end" class="w-40">
            <DropdownMenu.Item onclick={openRename}>
              <Pencil class="size-4" />
              {$t('rename')}
            </DropdownMenu.Item>
            <DropdownMenu.Item variant="destructive" onclick={() => (deleteOpen = true)}>
              <Trash2 class="size-4" />
              {$t('delete')}
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      {/if}
    {/snippet}
  </EntityHeader>

  <!-- Members: the primary surface. Soft top accent, roster with avatars.
       Invites live here as a secondary action (dialog), not their own block. -->
  <Card.Root class="shadow-sm">
    <Card.Header>
      <div class="space-y-1">
        <div class="flex items-center gap-1.5">
          <Card.Title>{$t('members')}</Card.Title>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              <button
                type="button"
                class="text-muted-foreground hover:text-foreground transition-colors"
                aria-label={$t('what_can_each_role_do')}
              >
                <Info class="size-4" />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="start" class="w-80 p-3">
              <RoleHint scope="team" />
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </div>
        <Card.Description>{$t('who_s_in_this_team_and_what_role_they_have')}</Card.Description>
      </div>
      {#if canManageMembers}
        <Card.Action class="flex items-center gap-2">
          <Button variant="outline" size="sm" class="gap-1.5" onclick={openInvite}>
            <Link2 class="size-4" />
            {$t('invite_by_link')}
          </Button>
          <Button
            variant="default"
            size="sm"
            class="gap-1.5"
            onclick={openAdd}
            disabled={candidates.length === 0}
          >
            <UserPlus class="size-4" />
            {$t('add_member')}
          </Button>
        </Card.Action>
      {/if}
    </Card.Header>
    <Card.Content class="space-y-4">
      {#if canManageMembers && pendingInvites.length > 0}
        <div class="bg-muted/30 space-y-2 rounded-lg border p-3">
          <p class="text-muted-foreground text-xs font-medium tracking-wide uppercase">
            {$t('pending_invites')}
          </p>
          {#each pendingInvites as invite (invite.token)}
            <div class="space-y-2">
              <div class="flex items-center justify-between gap-2">
                <div class="flex min-w-0 items-center gap-2 text-sm">
                  <Badge
                    variant="outline"
                    class={invite.role === 'admin' ? 'text-primary capitalize' : 'capitalize'}
                  >
                    {$t(invite.role)}
                  </Badge>
                  {#if invite.email}
                    <span class="truncate">{invite.email}</span>
                  {:else}
                    <span class="text-muted-foreground">{$t('anyone_with_the_link')}</span>
                  {/if}
                  <span class="text-muted-foreground shrink-0 text-xs"
                    >{$t('expires')} {formatRelative(invite.expires_at, $locale)}</span
                  >
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-8 shrink-0"
                  onclick={() => revokeInvite(invite)}
                  aria-label={$t('revoke_invite')}
                >
                  <Trash2 class="text-destructive size-4" />
                </Button>
              </div>
              <CopyField value={invite.link} label={$t('invite_link')} />
            </div>
          {/each}
        </div>
      {/if}

      {#if team.members.length === 0}
        <div class="flex flex-col items-center gap-1.5 py-8 text-center">
          <span class="bg-muted text-primary grid size-11 place-items-center rounded-xl">
            <UserPlus class="size-5" />
          </span>
          <p class="font-medium">{$t('no_members_yet')}</p>
          <p class="text-muted-foreground max-w-xs text-sm">
            {$t('add_a_teammate_or_share_an_invite_link_to_give_access_to_this_team_s_projects')}
          </p>
          {#if canManageMembers}
            <div class="mt-3 flex items-center gap-2">
              <Button variant="outline" size="sm" class="gap-1.5" onclick={openInvite}>
                <Link2 class="size-4" />
                {$t('invite_by_link')}
              </Button>
              <Button
                size="sm"
                class="gap-1.5"
                onclick={openAdd}
                disabled={candidates.length === 0}
              >
                <UserPlus class="size-4" />
                {$t('add_member')}
              </Button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="divide-border/60 divide-y">
          {#each team.members as member (member.id)}
            <div class="flex items-center gap-3 py-3 first:pt-0 last:pb-0">
              <Avatar name={member.display_name} id={member.id} />
              <div class="min-w-0 flex-1">
                <p class="truncate font-medium">{member.display_name}</p>
                <p class="text-muted-foreground truncate text-sm">{member.email}</p>
              </div>
              {#if canManageMembers}
                <NativeSelect
                  aria-label={$t('team_role_for', { values: { name: member.display_name } })}
                  value={member.role}
                  disabled={busyMemberId === member.id}
                  onchange={(e) => changeRole(member.id, e.currentTarget.value as TeamRole)}
                  class="shrink-0"
                >
                  <option value="admin">{$t('admin')}</option>
                  <option value="contributor" disabled={demoteBlocked(member)}
                    >{$t('contributor')}</option
                  >
                </NativeSelect>
                <Button
                  variant="ghost"
                  size="icon"
                  class="text-destructive hover:text-destructive size-8 shrink-0"
                  title={$t('remove_member')}
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
                  {$t(member.role)}
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
      <Card.Title class="text-base">{$t('projects')}</Card.Title>
      <Card.Description>{$t('projects_this_team_can_access')}</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if team.projects.length === 0}
        <div class="flex flex-col items-center gap-2 py-6 text-center">
          <FolderKanban class="text-muted-foreground size-7" />
          <p class="text-muted-foreground text-sm">{$t('no_projects_assigned_to_this_team')}</p>
        </div>
      {:else}
        <div class="flex flex-wrap gap-2">
          {#each team.projects as project (project.id)}
            <a
              href={`/projects/${project.id}`}
              class="hover:border-primary/40 hover:bg-muted/50 group inline-flex items-center gap-2 rounded-lg border py-1.5 pr-3 pl-1.5 text-sm transition-colors"
            >
              <Avatar name={project.name} id={project.id} shape="tile" size="sm" />
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
      <Dialog.Title>{$t('rename_team')}</Dialog.Title>
      <Dialog.Description>{$t('update_the_display_name_for_this_team')}</Dialog.Description>
    </Dialog.Header>
    <form onsubmit={renameTeam} class="space-y-4">
      <div class="space-y-2">
        <label for="rename-team" class="text-sm font-medium">{$t('name')}</label>
        <Input id="rename-team" bind:value={renameName} required />
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (renameOpen = false)}
          >{$t('cancel')}</Button
        >
        <Button type="submit" disabled={renaming || renameName.trim().length === 0}>
          {renaming ? $t('saving') : $t('save')}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete confirmation (admin) -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{$t('delete_team')}</Dialog.Title>
      <Dialog.Description>
        {$t('delete_team_confirm', { values: { name: team.name } })}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button type="button" variant="outline" onclick={() => (deleteOpen = false)}
        >{$t('cancel')}</Button
      >
      <Button variant="destructive" disabled={deleting} onclick={deleteTeam}>
        {deleting ? $t('deleting') : $t('delete_team')}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Invite by link dialog -->
<Dialog.Root bind:open={inviteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{$t('invite_by_link')}</Dialog.Title>
      <Dialog.Description>
        {$t('generate_a_link_to_add_someone_to_this_team_the_link_works_even_without_email_if')}
      </Dialog.Description>
    </Dialog.Header>
    <form onsubmit={createInvite} class="space-y-4">
      <div class="space-y-2">
        <label for="invite-email" class="text-sm font-medium">{$t('email_optional')}</label>
        <Input
          id="invite-email"
          type="email"
          placeholder="teammate@example.com"
          bind:value={inviteEmail}
        />
      </div>
      <div class="space-y-2">
        <label for="invite-role" class="text-sm font-medium">{$t('role')}</label>
        <NativeSelect id="invite-role" bind:value={inviteRole} class="w-full">
          <option value="contributor">{$t('contributor')}</option>
          <option value="admin">{$t('admin')}</option>
        </NativeSelect>
        <RoleHint scope="team" />
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (inviteOpen = false)}
          >{$t('cancel')}</Button
        >
        <Button type="submit" disabled={creatingInvite}>
          {creatingInvite ? $t('creating') : $t('create_invite')}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>

<!-- Add member dialog -->
<Dialog.Root bind:open={addOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{$t('add_member')}</Dialog.Title>
      <Dialog.Description>{$t('select_a_user_and_a_role_to_add_to_this_team')}</Dialog.Description>
    </Dialog.Header>
    <form onsubmit={addMember} class="space-y-4">
      <div class="space-y-2">
        <label for="add-member" class="text-sm font-medium">{$t('user')}</label>
        {#if candidates.length === 0}
          <p class="text-muted-foreground text-sm">{$t('all_users_are_already_members')}</p>
        {:else}
          <NativeSelect id="add-member" bind:value={selectedUserId} class="w-full">
            {#each candidates as user (user.id)}
              <option value={user.id}>{user.display_name} ({user.email})</option>
            {/each}
          </NativeSelect>
        {/if}
      </div>
      <div class="space-y-2">
        <label for="add-member-role" class="text-sm font-medium">{$t('role')}</label>
        <NativeSelect id="add-member-role" bind:value={selectedRole} class="w-full">
          <option value="contributor">{$t('contributor')}</option>
          <option value="admin">{$t('admin')}</option>
        </NativeSelect>
        <RoleHint scope="team" />
      </div>
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (addOpen = false)}
          >{$t('cancel')}</Button
        >
        <Button type="submit" disabled={adding || !selectedUserId}>
          {adding ? $t('adding') : $t('add_member')}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
