<script lang="ts">
  import { t } from '$lib/i18n';
  import { Badge } from '$lib/components/ui/badge';
  import { cn } from '$lib/utils';
  import type { IssueStatus } from '$lib/api';

  // Maps an issue status to a labelled, colour-coded badge.
  let { status, class: className }: { status: IssueStatus; class?: string } = $props();

  const config: Record<
    IssueStatus,
    { label: string; variant: 'outline' | 'secondary' | 'destructive'; class?: string }
  > = $derived({
    unresolved: { label: $t('status_unresolved'), variant: 'destructive' },
    resolved: {
      label: $t('status_resolved'),
      variant: 'outline',
      class: 'border-green-600/40 text-green-700 dark:text-green-400'
    },
    muted: { label: $t('status_muted'), variant: 'secondary' }
  });

  const c = $derived(config[status]);
</script>

<Badge variant={c.variant} class={cn(c.class, className)}>{c.label}</Badge>
