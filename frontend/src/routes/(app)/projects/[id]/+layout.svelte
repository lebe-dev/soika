<script lang="ts">
  import type { Snippet } from 'svelte';
  import { page } from '$app/stores';
  import { cn } from '$lib/utils';
  import type { LayoutData } from './$types';
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import FolderGit2 from '@lucide/svelte/icons/folder-git-2';
  import ListFilter from '@lucide/svelte/icons/list-filter';
  import Terminal from '@lucide/svelte/icons/terminal';
  import Settings from '@lucide/svelte/icons/settings';
  import BellOff from '@lucide/svelte/icons/bell-off';

  // Project area shell header with project name/DSN status and
  // sub-navigation across Issues, SDK Setup, and Settings. The project is loaded
  // once in +layout.ts and shared with every child route.
  let { data, children }: { data: LayoutData; children: Snippet } = $props();

  const project = $derived(data.project);
  const base = $derived(`/projects/${project.id}`);

  type Tab = {
    href: string;
    label: string;
    icon: typeof ListFilter;
    match: (p: string) => boolean;
  };
  const tabs = $derived<Tab[]>([
    {
      href: base,
      label: 'Issues',
      icon: ListFilter,
      match: (p) => p === base || p.startsWith(`${base}/issues`)
    },
    {
      href: `${base}/setup`,
      label: 'SDK Setup',
      icon: Terminal,
      match: (p) => p.startsWith(`${base}/setup`)
    },
    {
      href: `${base}/settings`,
      label: 'Settings',
      icon: Settings,
      match: (p) => p.startsWith(`${base}/settings`)
    }
  ]);

  const path = $derived($page.url.pathname);
</script>

<div class="space-y-6">
  <a
    href="/"
    class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 text-sm transition-colors"
  >
    <ArrowLeft class="size-4" />
    Back to dashboard
  </a>

  <div class="flex flex-wrap items-center gap-3">
    <div class="bg-muted/40 flex size-10 items-center justify-center rounded-lg border">
      <FolderGit2 class="text-muted-foreground size-5" />
    </div>
    <div class="min-w-0">
      <div class="flex items-center gap-2">
        <h1 class="truncate text-2xl font-semibold tracking-tight">{project.name}</h1>
        {#if project.muted}
          <a
            href={`${base}/settings`}
            title="Notifications muted — open settings"
            aria-label="Notifications muted — open settings"
            class="text-orange-500 transition-colors hover:text-orange-600"
          >
            <BellOff class="size-5" />
          </a>
        {/if}
      </div>
      <p class="text-muted-foreground font-mono text-xs">{project.slug}</p>
    </div>
  </div>

  <nav class="flex items-center gap-1 border-b text-sm">
    {#each tabs as tab (tab.href)}
      <a
        href={tab.href}
        class={cn(
          '-mb-px flex items-center gap-1.5 border-b-2 px-3 py-2 font-medium transition-colors',
          tab.match(path)
            ? 'border-primary text-foreground'
            : 'text-muted-foreground hover:text-foreground border-transparent'
        )}
      >
        <tab.icon class="size-4" />
        {tab.label}
      </a>
    {/each}
  </nav>

  {@render children()}
</div>
