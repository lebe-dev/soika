<script lang="ts">
  import { Badge } from '$lib/components/ui/badge';
  import type { IssueStatus } from '$lib/api';

  // Maps an issue status (MVP §8.1) to a labelled, colour-coded badge.
  let { status }: { status: IssueStatus } = $props();

  const config: Record<
    IssueStatus,
    { label: string; variant: 'outline' | 'secondary' | 'destructive'; class?: string }
  > = {
    unresolved: { label: 'Unresolved', variant: 'destructive' },
    resolved: {
      label: 'Resolved',
      variant: 'outline',
      class: 'border-green-600/40 text-green-700 dark:text-green-400'
    },
    muted: { label: 'Muted', variant: 'secondary' }
  };

  const c = $derived(config[status]);
</script>

<Badge variant={c.variant} class={c.class}>{c.label}</Badge>
