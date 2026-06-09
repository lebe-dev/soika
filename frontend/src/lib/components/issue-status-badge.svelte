<script lang="ts">
  import { Badge } from '$lib/components/ui/badge';
  import { cn } from '$lib/utils';
  import type { IssueStatus } from '$lib/api';

  // Maps an issue status to a labelled, colour-coded badge.
  let { status, class: className }: { status: IssueStatus; class?: string } = $props();

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

<Badge variant={c.variant} class={cn(c.class, className)}>{c.label}</Badge>
