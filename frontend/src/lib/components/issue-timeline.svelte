<script lang="ts">
  import { formatRelative, formatDateTime } from '$lib/format';

  // Compact lifetime strip for an issue: "first seen → last seen". The filled
  // segment of the track is the share of the issue's lifetime during which it
  // was active — (lastSeen − firstSeen) / (now − firstSeen) — so the remaining
  // (empty) tail visualises how long it has been quiet since the last event.
  let { firstSeen, lastSeen }: { firstSeen: string; lastSeen: string } = $props();

  const activePercent = $derived.by(() => {
    const first = Date.parse(firstSeen);
    const last = Date.parse(lastSeen);
    const now = Date.now();
    if (Number.isNaN(first) || Number.isNaN(last)) return 100;
    const span = now - first;
    if (span <= 0) return 100;
    const active = Math.min(Math.max(last - first, 0), span);
    // Floor at a sliver so a single-event issue still reads as a marker.
    return Math.max((active / span) * 100, 2);
  });
</script>

<div class="space-y-2.5">
  <div class="flex items-end justify-between gap-3">
    <div>
      <div class="text-muted-foreground text-xs tracking-wide uppercase">First seen</div>
      <div class="text-sm font-medium" title={formatDateTime(firstSeen)}>
        {formatRelative(firstSeen)}
      </div>
    </div>
    <div class="text-right">
      <div class="text-muted-foreground text-xs tracking-wide uppercase">Last seen</div>
      <div class="text-sm font-medium" title={formatDateTime(lastSeen)}>
        {formatRelative(lastSeen)}
      </div>
    </div>
  </div>

  <div class="bg-muted relative h-1.5 w-full overflow-hidden rounded-full">
    <div
      class="bg-primary/60 absolute inset-y-0 left-0 rounded-full"
      style:width={`${activePercent}%`}
    ></div>
  </div>
</div>
