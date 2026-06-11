<script lang="ts">
  import {
    projects as projectsApi,
    errorMessage,
    type Id,
    type TagMatch,
    type TagMuteRule
  } from '$lib/api';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as Dialog from '$lib/components/ui/dialog';
  import { toast } from '$lib/components/ui/sonner';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';

  // A flexible form for creating a project-level "mute by tags" rule: an
  // optional label plus one-or-more `key=value` pairs (AND). Reused by the issue
  // dropdown (prefilled from the current event's tags) and project settings.
  let {
    open = $bindable(),
    projectId,
    prefillTags = [],
    onCreated
  }: {
    open: boolean;
    projectId: Id;
    prefillTags?: TagMatch[];
    onCreated?: (rule: TagMuteRule) => void;
  } = $props();

  // A tag row carries a stable `id` so `{#each}` can key by identity rather than
  // array index — index keys mis-associate two-way-bound inputs when a middle
  // row is removed. The id is local-only and stripped before submit.
  type Row = TagMatch & { id: number };
  let nextRowId = 0;
  const makeRow = (tag: TagMatch = { key: '', value: '' }): Row => ({ ...tag, id: nextRowId++ });

  let name = $state('');
  let rows = $state<Row[]>([makeRow()]);
  let saving = $state(false);

  // Reset the form to the (possibly prefilled) initial state whenever the dialog
  // opens, so a previous edit never leaks into the next use.
  $effect(() => {
    if (open) {
      name = '';
      rows = prefillTags.length > 0 ? prefillTags.map((t) => makeRow(t)) : [makeRow()];
    }
  });

  const validRows = $derived(rows.filter((r) => r.key.trim() && r.value.trim()));
  const canSubmit = $derived(validRows.length > 0 && !saving);

  function addRow() {
    rows = [...rows, makeRow()];
  }

  function removeRow(id: number) {
    rows = rows.filter((r) => r.id !== id);
    if (rows.length === 0) rows = [makeRow()];
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const tags = validRows.map((r) => ({ key: r.key.trim(), value: r.value.trim() }));
    if (tags.length === 0) return;

    // Reject duplicate keys client-side (the backend enforces this too).
    const keys = new Set<string>();
    for (const t of tags) {
      if (keys.has(t.key)) {
        toast.error(`Duplicate tag key: ${t.key}`);
        return;
      }
      keys.add(t.key);
    }

    saving = true;
    try {
      const trimmedName = name.trim();
      const rule = await projectsApi.createMuteRule(projectId, {
        name: trimmedName || undefined,
        tags
      });
      toast.success('Tag-mute rule created');
      open = false;
      onCreated?.(rule);
    } catch (err) {
      toast.error(errorMessage(err, 'Could not create rule'));
    } finally {
      saving = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Mute by tags</Dialog.Title>
      <Dialog.Description>
        Suppress notifications for events whose tags match all of the pairs below. Events are still
        ingested and counted — only alerts are silenced.
      </Dialog.Description>
    </Dialog.Header>

    <form onsubmit={submit} class="space-y-4">
      <div class="space-y-1">
        <label for="rule-name" class="text-sm font-medium">Name (optional)</label>
        <Input id="rule-name" bind:value={name} placeholder="e.g. staging noise" />
      </div>

      <div class="space-y-2">
        <span class="text-sm font-medium">Tags (all must match)</span>
        {#each rows as row (row.id)}
          <div class="flex items-center gap-2">
            <Input bind:value={row.key} placeholder="key (e.g. environment)" class="flex-1" />
            <span class="text-muted-foreground">=</span>
            <Input bind:value={row.value} placeholder="value (e.g. staging)" class="flex-1" />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="Remove tag"
              onclick={() => removeRow(row.id)}
            >
              <X class="size-4" />
            </Button>
          </div>
        {/each}
        <Button type="button" variant="outline" size="sm" onclick={addRow} class="gap-2">
          <Plus class="size-4" />
          Add tag
        </Button>
      </div>

      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
        <Button type="submit" disabled={!canSubmit}>
          {saving ? 'Creating…' : 'Create rule'}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
