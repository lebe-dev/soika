<script lang="ts">
  import * as Card from '$lib/components/ui/card';
  import { extractContext, isContextEmpty, type KeyValue } from '$lib/event-context';
  import Globe from '@lucide/svelte/icons/globe';
  import User from '@lucide/svelte/icons/user';
  import Tag from '@lucide/svelte/icons/tag';
  import Layers from '@lucide/svelte/icons/layers';
  import Braces from '@lucide/svelte/icons/braces';

  // Renders the non-stacktrace context of an event: a headline row of chips
  // (browser, OS, IP, environment, release) plus per-interface sections (user,
  // tags, request, device/runtime contexts, additional data). Mirrors what
  // Sentry shows beneath the stacktrace. The browser/OS/IP shown here are filled
  // in server-side at ingest when the SDK didn't send them (src/ingest/enrich.rs).
  let { payload }: { payload: unknown } = $props();

  const ctx = $derived(extractContext(payload));
</script>

{#if !isContextEmpty(ctx)}
  <div class="space-y-4">
    <!-- Headline chips: the at-a-glance "where / who / what build". -->
    {#if ctx.highlights.length > 0}
      <div class="flex flex-wrap gap-2">
        {#each ctx.highlights as chip (chip.key)}
          <div class="bg-muted/40 inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1">
            <span class="text-muted-foreground text-xs tracking-wide uppercase">{chip.key}</span>
            <span class="font-mono text-xs font-medium break-all">{chip.value}</span>
          </div>
        {/each}
      </div>
    {/if}

    <div class="grid gap-4 md:grid-cols-2">
      {#snippet kvTable(rows: KeyValue[])}
        <dl class="divide-y text-sm">
          {#each rows as row (row.key)}
            <div class="grid grid-cols-[minmax(7rem,auto)_1fr] gap-3 py-1.5">
              <dt class="text-muted-foreground truncate" title={row.key}>{row.key}</dt>
              <dd class="font-mono text-xs break-all">{row.value}</dd>
            </div>
          {/each}
        </dl>
      {/snippet}

      {#snippet section(title: string, Icon: typeof User)}
        <div class="text-muted-foreground flex items-center gap-2 text-sm font-medium">
          <Icon class="size-4" />
          {title}
        </div>
      {/snippet}

      {#if ctx.user.length > 0}
        <Card.Root>
          <Card.Content class="space-y-2 p-4">
            {@render section('User', User)}
            {@render kvTable(ctx.user)}
          </Card.Content>
        </Card.Root>
      {/if}

      {#if ctx.tags.length > 0}
        <Card.Root>
          <Card.Content class="space-y-2 p-4">
            {@render section('Tags', Tag)}
            {@render kvTable(ctx.tags)}
          </Card.Content>
        </Card.Root>
      {/if}

      {#if ctx.request}
        <Card.Root>
          <Card.Content class="space-y-2 p-4">
            {@render section('Request', Globe)}
            <dl class="divide-y text-sm">
              {#if ctx.request.url}
                <div class="grid grid-cols-[minmax(7rem,auto)_1fr] gap-3 py-1.5">
                  <dt class="text-muted-foreground">URL</dt>
                  <dd class="font-mono text-xs break-all">
                    {ctx.request.method ? `${ctx.request.method} ` : ''}{ctx.request.url}
                  </dd>
                </div>
              {/if}
              {#if ctx.request.query}
                <div class="grid grid-cols-[minmax(7rem,auto)_1fr] gap-3 py-1.5">
                  <dt class="text-muted-foreground">Query</dt>
                  <dd class="font-mono text-xs break-all">{ctx.request.query}</dd>
                </div>
              {/if}
              {#each ctx.request.headers as header (header.key)}
                <div class="grid grid-cols-[minmax(7rem,auto)_1fr] gap-3 py-1.5">
                  <dt class="text-muted-foreground truncate" title={header.key}>{header.key}</dt>
                  <dd class="font-mono text-xs break-all">{header.value}</dd>
                </div>
              {/each}
            </dl>
          </Card.Content>
        </Card.Root>
      {/if}

      {#if ctx.contexts.length > 0}
        <Card.Root>
          <Card.Content class="space-y-2 p-4">
            {@render section('Contexts', Layers)}
            <dl class="divide-y text-sm">
              {#each ctx.contexts as c (c.label)}
                <div class="grid grid-cols-[minmax(7rem,auto)_1fr] gap-3 py-1.5">
                  <dt class="text-muted-foreground truncate" title={c.label}>{c.label}</dt>
                  <dd class="font-mono text-xs break-all">
                    {[c.name, c.version].filter(Boolean).join(' ')}
                  </dd>
                </div>
              {/each}
            </dl>
          </Card.Content>
        </Card.Root>
      {/if}

      {#if ctx.additional.length > 0}
        <Card.Root>
          <Card.Content class="space-y-2 p-4">
            {@render section('Additional data', Braces)}
            {@render kvTable(ctx.additional)}
          </Card.Content>
        </Card.Root>
      {/if}
    </div>

    {#if ctx.sdk}
      <p class="text-muted-foreground text-xs">
        Reported by <span class="font-mono">{ctx.sdk}</span>
      </p>
    {/if}
  </div>
{/if}
