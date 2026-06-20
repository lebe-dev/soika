<script lang="ts">
  import { untrack } from 'svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Tabs from '$lib/components/ui/tabs';
  import CopyField from '$lib/components/copy-field.svelte';
  import PageTitle from '$lib/components/page-title.svelte';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import { sdkSnippets } from '$lib/sdk-snippets';
  import type { PageData } from './$types';

  // SDK Setup — per-project, language-aware snippets showing the DSN and minimal
  // init code (Go, Rust, Svelte/JS, generic) to start sending events. Snippets
  // are derived on the client from the project's DSN (loaded once by the parent
  // layout), so opening this tab fires no requests.
  let { data }: { data: PageData } = $props();

  const dsn = $derived(data.project.dsn);
  const snippets = $derived(sdkSnippets(data.project.dsn, data.project.dsn_public_key));
  let active = $state(untrack(() => 'go'));
</script>

<PageTitle title={`${data.project.name} · SDK Setup`} />

<div class="space-y-6">
  <Card.Root class="ring-foreground/15 dark:ring-foreground/30 shadow-sm">
    <Card.Header>
      <Card.Title class="flex items-center gap-2">
        <KeyRound class="text-muted-foreground size-4" />
        DSN
      </Card.Title>
      <Card.Description>
        Point your Sentry-compatible SDK at this DSN to start sending events to this project.
      </Card.Description>
    </Card.Header>
    <Card.Content>
      <CopyField value={dsn} label="DSN" />
    </Card.Content>
  </Card.Root>

  <Card.Root>
    <Card.Header>
      <Card.Title>Initialize the SDK</Card.Title>
      <Card.Description>
        Copy a snippet for your language. soika is a drop-in ingestion endpoint for the Sentry SDK.
      </Card.Description>
    </Card.Header>
    <Card.Content>
      <Tabs.Root bind:value={active}>
        <Tabs.List>
          {#each snippets as snippet (snippet.language)}
            <Tabs.Trigger value={snippet.language}>{snippet.label}</Tabs.Trigger>
          {/each}
        </Tabs.List>
        {#each snippets as snippet (snippet.language)}
          <Tabs.Content value={snippet.language}>
            <CopyField value={snippet.code} label={`${snippet.label} snippet`} block />
          </Tabs.Content>
        {/each}
      </Tabs.Root>
    </Card.Content>
  </Card.Root>

  <Card.Root>
    <Card.Header>
      <Card.Title>Next steps</Card.Title>
    </Card.Header>
    <Card.Content>
      <ul class="text-muted-foreground ml-4 list-disc space-y-1 text-sm">
        <li>Trigger an error in your application to send your first event.</li>
        <li>New events appear on the project's Issues tab, grouped by fingerprint.</li>
        <li>
          soika renders stacktraces exactly as your SDK reports them — no source maps required.
        </li>
      </ul>
    </Card.Content>
  </Card.Root>
</div>
