<script lang="ts">
  import { goto } from '$app/navigation';
  import { projects as projectsApi, errorMessage } from '$lib/api';
  import type { Project } from '$lib/api';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import * as Card from '$lib/components/ui/card';
  import * as Dialog from '$lib/components/ui/dialog';
  import { toast } from '$lib/components/ui/sonner';
  import FolderPlus from '@lucide/svelte/icons/folder-plus';
  import VolumeX from '@lucide/svelte/icons/volume-x';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  const overviews = $derived(data.overviews);
  const teams = $derived(data.teams);

  // --- Create-project dialog state ---
  let createOpen = $state(false);
  let name = $state('');
  let teamId = $state('');
  let submitting = $state(false);

  // Default the team selection to the first available team when the dialog opens.
  $effect(() => {
    if (createOpen && !teamId && teams.length > 0) {
      teamId = teams[0].id;
    }
  });

  const canCreate = $derived(teams.length > 0);

  async function createProject(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim() || !teamId) return;
    submitting = true;
    try {
      const project: Project = await projectsApi.create({ name: name.trim(), team_id: teamId });
      toast.success(`Project “${project.name}” created`);
      createOpen = false;
      name = '';
      await goto(`/projects/${project.id}`);
    } catch (err) {
      toast.error(errorMessage(err, 'Failed to create project'));
    } finally {
      submitting = false;
    }
  }
</script>

<PageTitle title="Dashboard" />

<div class="space-y-6">
  <div class="flex items-end justify-between gap-4">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Projects</h1>
      <p class="text-muted-foreground text-sm">
        {#if overviews.length === 0}
          No projects yet — create your first to start capturing errors.
        {:else}
          {overviews.length}
          {overviews.length === 1 ? 'project' : 'projects'} in your workspace.
        {/if}
      </p>
    </div>

    <Dialog.Root bind:open={createOpen}>
      <Dialog.Trigger>
        <Button class="gap-2">
          <FolderPlus class="size-4" />
          New project
        </Button>
      </Dialog.Trigger>
      <Dialog.Content class="sm:max-w-md">
        <Dialog.Header>
          <Dialog.Title>Create project</Dialog.Title>
          <Dialog.Description>
            A project owns a DSN and groups the errors your SDK sends.
          </Dialog.Description>
        </Dialog.Header>

        {#if !canCreate}
          <p class="text-muted-foreground text-sm">
            You need a team before creating a project.
            <a href="/teams" class="text-foreground underline-offset-4 hover:underline">
              Create a team
            </a>
            first.
          </p>
        {:else}
          <form onsubmit={createProject} class="space-y-4">
            <div class="space-y-2">
              <label for="project-name" class="text-sm font-medium">Name</label>
              <Input
                id="project-name"
                bind:value={name}
                placeholder="my-service"
                autocomplete="off"
                required
              />
            </div>
            <div class="space-y-2">
              <label for="project-team" class="text-sm font-medium">Team</label>
              <select
                id="project-team"
                bind:value={teamId}
                class="border-input focus-visible:ring-ring flex h-9 w-full rounded-md border bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:ring-1 focus-visible:outline-none"
              >
                {#each teams as team (team.id)}
                  <option value={team.id}>{team.name}</option>
                {/each}
              </select>
            </div>
            <Dialog.Footer>
              <Button type="submit" disabled={submitting || !name.trim()}>
                {submitting ? 'Creating…' : 'Create project'}
              </Button>
            </Dialog.Footer>
          </form>
        {/if}
      </Dialog.Content>
    </Dialog.Root>
  </div>

  {#if overviews.length === 0}
    <Card.Root>
      <Card.Header>
        <Card.Title>No projects</Card.Title>
        <Card.Description>
          Create a project to get a DSN and start sending events from your application.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        <Button class="gap-2" onclick={() => (createOpen = true)} disabled={!canCreate}>
          <FolderPlus class="size-4" />
          New project
        </Button>
        {#if !canCreate}
          <p class="text-muted-foreground mt-3 text-sm">
            No teams available yet —
            <a href="/teams" class="text-foreground underline-offset-4 hover:underline">
              create a team
            </a>
            to continue.
          </p>
        {/if}
      </Card.Content>
    </Card.Root>
  {:else}
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each overviews as { project, unresolvedCount } (project.id)}
        <a
          href={`/projects/${project.id}`}
          class="group focus-visible:ring-ring rounded-lg transition focus-visible:ring-2 focus-visible:outline-none"
        >
          <Card.Root class="group-hover:border-primary/50 h-full transition-colors">
            <Card.Header>
              <div class="flex items-start justify-between gap-2">
                <div class="min-w-0">
                  <Card.Title class="truncate">{project.name}</Card.Title>
                  <Card.Description class="truncate font-mono text-xs">
                    {project.slug}
                  </Card.Description>
                </div>
                {#if project.muted}
                  <Badge variant="secondary" class="shrink-0 gap-1">
                    <VolumeX class="size-3" />
                    Muted
                  </Badge>
                {/if}
              </div>
            </Card.Header>
            <Card.Content>
              {#if unresolvedCount === 0}
                <p class="text-muted-foreground flex items-center gap-1.5 text-sm">
                  <CircleCheck class="size-4 text-emerald-500" />
                  No unresolved issues
                </p>
              {:else}
                <p class="flex items-center gap-1.5 text-sm font-medium">
                  <CircleAlert class="text-destructive size-4" />
                  {unresolvedCount}
                  {unresolvedCount === 1 ? 'unresolved issue' : 'unresolved issues'}
                </p>
              {/if}
            </Card.Content>
          </Card.Root>
        </a>
      {/each}
    </div>
  {/if}
</div>
