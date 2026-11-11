<script lang="ts">
  import { isDisabledProject } from '$lib/dashboard';
  import { t } from '$lib/i18n';
  import { untrack } from 'svelte';
  import * as Card from '$lib/components/ui/card';
  import * as Tabs from '$lib/components/ui/tabs';
  import CopyField from '$lib/components/copy-field.svelte';
  import PageTitle from '$lib/components/page-title.svelte';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { toast } from '$lib/components/ui/sonner';
  import { invalidateAll } from '$app/navigation';
  import { projects as projectsApi, errorMessage } from '$lib/api';
  import { reportUnexpected } from '$lib/report';
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

  // Rotating the DSN is a project-admin action (team Admin or instance
  // Owner/Manager); the backend enforces the same rule.
  const canRegenerate = $derived(
    data.effectiveRole === 'admin' && !isDisabledProject(data.project)
  );
  let regenerateOpen = $state(false);
  let regenerating = $state(false);

  async function regenerateDsn() {
    regenerating = true;
    try {
      await projectsApi.regenerateDsn(data.project.id);
      regenerateOpen = false;
      toast.success($t('dsn_regenerated'));
      await invalidateAll();
    } catch (err) {
      toast.error(errorMessage(err, $t('failed_to_regenerate_dsn')));
      reportUnexpected(err);
    } finally {
      regenerating = false;
    }
  }
</script>

<PageTitle title={`${data.project.name} · ${$t('sdk_setup')}`} />

<div class="space-y-6">
  <Card.Root class="ring-foreground/15 dark:ring-foreground/30 shadow-sm">
    <Card.Header>
      <Card.Title class="flex items-center gap-2">
        <KeyRound class="text-muted-foreground size-4" />
        DSN
      </Card.Title>
      <Card.Description>
        {$t('point_your_sentry_compatible_sdk_at_this_dsn_to_start_sending_events_to_this_pro')}
      </Card.Description>
      {#if canRegenerate}
        <Card.Action>
          <Button
            variant="outline"
            size="sm"
            class="gap-1.5"
            onclick={() => (regenerateOpen = true)}
          >
            <RefreshCw class="size-4" />
            {$t('regenerate_dsn')}
          </Button>
        </Card.Action>
      {/if}
    </Card.Header>
    <Card.Content>
      <CopyField value={dsn} label="DSN" />
    </Card.Content>
  </Card.Root>

  <Card.Root>
    <Card.Header>
      <Card.Title>{$t('initialize_the_sdk')}</Card.Title>
      <Card.Description>
        {$t('copy_a_snippet_for_your_language_soika_is_a_drop_in_ingestion_endpoint_for_the_s')}
      </Card.Description>
    </Card.Header>
    <Card.Content>
      <Tabs.Root bind:value={active}>
        <Tabs.List
          class="[&_[data-slot=tabs-trigger]:not(:first-child)]:border-l-border [&_[data-slot=tabs-trigger]]:text-xs [&_[data-slot=tabs-trigger]:not(:first-child)]:rounded-l-none"
        >
          {#each snippets as snippet (snippet.language)}
            <Tabs.Trigger value={snippet.language}>{snippet.label}</Tabs.Trigger>
          {/each}
        </Tabs.List>
        {#each snippets as snippet (snippet.language)}
          <Tabs.Content value={snippet.language}>
            <CopyField
              value={snippet.code}
              label={$t('snippet_label', { values: { name: snippet.label } })}
              block
            />
          </Tabs.Content>
        {/each}
      </Tabs.Root>
    </Card.Content>
  </Card.Root>

  <Card.Root>
    <Card.Header>
      <Card.Title>{$t('next_steps')}</Card.Title>
    </Card.Header>
    <Card.Content>
      <ul class="text-muted-foreground ml-4 list-disc space-y-1 text-sm">
        <li>{$t('trigger_an_error_in_your_application_to_send_your_first_event')}</li>
        <li>{$t('new_events_appear_on_the_project_s_issues_tab_grouped_by_fingerprint')}</li>
        <li>
          {$t('soika_renders_stacktraces_exactly_as_your_sdk_reports_them_no_source_maps_requir')}
        </li>
      </ul>
    </Card.Content>
  </Card.Root>
</div>

<Dialog.Root bind:open={regenerateOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{$t('regenerate_dsn_title')}</Dialog.Title>
      <Dialog.Description>
        {$t('regenerate_dsn_confirm', { values: { name: data.project.name } })}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button type="button" variant="outline" onclick={() => (regenerateOpen = false)}>
        {$t('cancel')}
      </Button>
      <Button variant="destructive" disabled={regenerating} onclick={regenerateDsn}>
        {regenerating ? $t('regenerating') : $t('regenerate_dsn')}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
