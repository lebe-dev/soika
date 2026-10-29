<script lang="ts" module>
  // Identity chip used across the app: people are round, entities (teams,
  // projects) are rounded tiles. Passing `id` picks a deterministic tint so a
  // roster reads at a glance; without it the chip uses the primary tint.
  export type AvatarShape = 'circle' | 'tile';
  export type AvatarSize = 'sm' | 'md' | 'lg';

  const SIZES: Record<AvatarShape, Record<AvatarSize, string>> = {
    circle: {
      sm: 'size-6 rounded-full text-[10px]',
      md: 'size-9 rounded-full text-xs',
      lg: 'size-12 rounded-full text-base'
    },
    tile: {
      sm: 'size-6 rounded-md text-[10px]',
      md: 'size-9 rounded-[10px] text-xs',
      lg: 'size-12 rounded-[13px] text-[15px]'
    }
  };
</script>

<script lang="ts">
  import { avatarTint, initials } from '$lib/avatar';
  import { cn } from '$lib/utils';

  let {
    name,
    id,
    shape = 'circle',
    size = 'md',
    class: className
  }: {
    name: string;
    id?: string;
    shape?: AvatarShape;
    size?: AvatarSize;
    class?: string;
  } = $props();
</script>

<span
  class={cn(
    'grid shrink-0 place-items-center font-semibold tracking-[0.02em] shadow-[inset_0_0_0_1px_rgb(20_23_28/0.04)] select-none',
    SIZES[shape][size],
    id ? avatarTint(id) : 'bg-primary/10 text-primary',
    className
  )}
  aria-hidden="true"
>
  {initials(name)}
</span>
