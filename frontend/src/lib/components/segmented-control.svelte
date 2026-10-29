<script lang="ts" generics="T extends string">
  // Pill-in-track switch for mutually exclusive view options (sort order,
  // status filter). The active option lifts onto a card-coloured chip.
  import { cn } from '$lib/utils';

  let {
    options,
    value,
    onselect,
    label,
    class: className
  }: {
    options: { value: T; label: string }[];
    value: T;
    onselect: (value: T) => void;
    label: string;
    class?: string;
  } = $props();
</script>

<div
  role="group"
  aria-label={label}
  class={cn('bg-muted inline-flex h-8 gap-0.5 rounded-[10px] p-[3px] text-[13px]', className)}
>
  {#each options as option (option.value)}
    <button
      type="button"
      aria-pressed={value === option.value}
      onclick={() => onselect(option.value)}
      class={cn(
        'flex items-center rounded-[7px] px-3 whitespace-nowrap transition-colors',
        value === option.value
          ? 'bg-card text-foreground dark:bg-accent font-medium shadow-[0_1px_2px_rgb(20_23_28/0.08),0_0_0_1px_rgb(20_23_28/0.04)]'
          : 'text-muted-foreground hover:text-foreground'
      )}
    >
      {option.label}
    </button>
  {/each}
</div>
