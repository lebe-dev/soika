<script lang="ts">
  import { untrack } from 'svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Tabs from '$lib/components/ui/tabs';
  import CopyField from '$lib/components/copy-field.svelte';
  import PageTitle from '$lib/components/page-title.svelte';
  import type { PageData } from './$types';

  // SDK Setup — per-project, language-aware snippets showing the DSN and minimal
  // init code (Go, Rust, Svelte/JS, generic) to start sending events.
  let { data }: { data: PageData } = $props();

  const setup = $derived(data.setup);
  const snippets = $derived(setup.snippets);
  let active = $state(untrack(() => data.setup.snippets[0]?.language ?? 'go'));
</script>

<PageTitle title={`${data.project.name} · SDK Setup`} />

<div class="space-y-6">
  <Card.Root>
    <Card.Header>
      <Card.Title>DSN</Card.Title>
      <Card.Description>
        Point your Sentry-compatible SDK at this DSN to start sending events to this project.
      </Card.Description>
    </Card.Header>
    <Card.Content>
      <CopyField value={setup.dsn} label="DSN" />
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
