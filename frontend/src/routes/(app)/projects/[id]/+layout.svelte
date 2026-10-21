<script lang="ts">
  import { t } from '$lib/i18n';
  import type { Snippet } from 'svelte';
  import { page } from '$app/stores';
  import { cn } from '$lib/utils';
  import type { LayoutData } from './$types';
  import ListFilter from '@lucide/svelte/icons/list-filter';
  import Terminal from '@lucide/svelte/icons/terminal';
  import Settings from '@lucide/svelte/icons/settings';
  import BellOff from '@lucide/svelte/icons/bell-off';
  import EntityHeader from '$lib/components/entity-header.svelte';
  import StatPill from '$lib/components/stat-pill.svelte';

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
      label: $t('issues'),
      icon: ListFilter,
      match: (p) => p === base || p.startsWith(`${base}/issues`)
    },
    {
      href: `${base}/setup`,
      label: $t('sdk_setup'),
      icon: Terminal,
      match: (p) => p.startsWith(`${base}/setup`)
    },
    {
      href: `${base}/settings`,
      label: $t('settings'),
      icon: Settings,
      match: (p) => p.startsWith(`${base}/settings`)
    }
  ]);

  const path = $derived($page.url.pathname);
</script>

<div class="space-y-6">
  <EntityHeader
    title={project.name}
    avatarId={project.id}
    back={{ href: '/', label: $t('projects') }}
  >
    {#snippet pills()}
      <StatPill class="font-mono">{project.slug}</StatPill>
      <StatPill>{$t('you_re')} <strong class="capitalize">{data.effectiveRole}</strong></StatPill>
      {#if project.muted}
        <StatPill
          href={`${base}/settings`}
          title={$t('notifications_muted_open_settings')}
          class="text-orange-600 dark:text-orange-400"
        >
          <BellOff />
          {$t('status_muted')}
        </StatPill>
      {/if}
    {/snippet}
  </EntityHeader>

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
