<script lang="ts">
  import { t } from '$lib/i18n';
  import { Button } from '$lib/components/ui/button';
  import { toast } from '$lib/components/ui/sonner';
  import { cn } from '$lib/utils';
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';

  // A read-only value with a copy-to-clipboard button. Used for DSNs and
  // copyable invite links. `block` renders a multi-line code
  // block (e.g. SDK snippets); otherwise a single-line inline field.
  let {
    value,
    label = '',
    block = false,
    class: className
  }: {
    value: string;
    label?: string;
    block?: boolean;
    class?: string;
  } = $props();

  let copied = $state(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      copied = true;
      toast.success($t('copied_label', { values: { label: label || $t('value') } }));
      setTimeout(() => (copied = false), 1500);
    } catch {
      toast.error($t('copy_failed_select_and_copy_manually'));
    }
  }
</script>

{#if block}
  <div class={cn('relative', className)}>
    <pre
      class="bg-muted/50 text-foreground overflow-x-auto rounded-md border p-4 pr-12 font-mono text-xs leading-relaxed">{value}</pre>
    <Button
      variant="ghost"
      size="icon"
      class="absolute top-2 right-2 size-7"
      onclick={copy}
      aria-label={$t('copy_label', { values: { label: label || $t('value') } })}
    >
      {#if copied}
        <Check class="size-3.5 text-green-600" />
      {:else}
        <Copy class="size-3.5" />
      {/if}
    </Button>
  </div>
{:else}
  <div class={cn('flex items-center gap-2', className)}>
    <code
      class="bg-muted/50 min-w-0 flex-1 truncate rounded-md border px-3 py-2 font-mono text-xs"
      title={value}>{value}</code
    >
    <Button
      variant="outline"
      size="icon"
      class="size-9 shrink-0"
      onclick={copy}
      aria-label={$t('copy_label', { values: { label: label || $t('value') } })}
    >
      {#if copied}
        <Check class="size-3.5 text-green-600" />
      {:else}
        <Copy class="size-3.5" />
      {/if}
    </Button>
  </div>
{/if}
