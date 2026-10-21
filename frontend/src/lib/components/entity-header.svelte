<script lang="ts">
  // Identity header for a detail page (team, project, profile): back link, an
  // avatar chip, the title, and soft stat pills that carry the at-a-glance
  // summary so the sections below can stay quiet. Actions sit on the right.
  import type { Snippet } from 'svelte';
  import Avatar, { type AvatarShape } from '$lib/components/avatar.svelte';
  import BackLink from '$lib/components/back-link.svelte';

  let {
    title,
    avatarName,
    avatarId,
    avatarShape = 'tile',
    back,
    titleAddon,
    pills,
    actions
  }: {
    title: string;
    /** Name the avatar initials are derived from; defaults to the title. */
    avatarName?: string;
    /** Tints the avatar like the matching list row, so it carries over. */
    avatarId?: string;
    avatarShape?: AvatarShape;
    back?: { href: string; label: string };
    titleAddon?: Snippet;
    pills?: Snippet;
    actions?: Snippet;
  } = $props();
</script>

<div class="space-y-6">
  {#if back}
    <div>
      <BackLink href={back.href} label={back.label} />
    </div>
  {/if}

  <div class="flex items-start justify-between gap-4">
    <div class="flex min-w-0 items-start gap-3.5">
      <Avatar name={avatarName ?? title} id={avatarId} shape={avatarShape} size="lg" />
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <h1 class="truncate text-2xl font-semibold tracking-tight">{title}</h1>
          {@render titleAddon?.()}
        </div>
        {#if pills}
          <div class="mt-2 flex flex-wrap items-center gap-2">
            {@render pills()}
          </div>
        {/if}
      </div>
    </div>
    {#if actions}
      <div class="flex shrink-0 items-center gap-2">
        {@render actions()}
      </div>
    {/if}
  </div>
</div>
