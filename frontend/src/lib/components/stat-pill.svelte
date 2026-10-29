<script lang="ts">
  // Soft summary pill for identity headers ("3 members", "you're Admin").
  // Emphasise the value with <strong>; an `href` turns the pill into a link.
  import type { Snippet } from 'svelte';
  import { cn } from '$lib/utils';

  let {
    href,
    title,
    class: className,
    children
  }: { href?: string; title?: string; class?: string; children: Snippet } = $props();

  const base =
    'bg-muted text-muted-foreground inline-flex h-6 items-center gap-1.5 rounded-md px-2 text-xs [&_strong]:text-foreground [&_strong]:font-medium [&_svg]:size-3.5';
</script>

{#if href}
  <a
    {href}
    {title}
    class={cn(base, 'hover:text-foreground hover:bg-accent transition-colors', className)}
  >
    {@render children()}
  </a>
{:else}
  <span {title} class={cn(base, className)}>
    {@render children()}
  </span>
{/if}
