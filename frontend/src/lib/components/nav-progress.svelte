<script lang="ts">
  // Thin top bar shown while a client-side navigation waits for its load
  // functions. It appears only after a short delay, so instant (preloaded)
  // navigations never flash it.
  import { navigating } from '$app/state';

  const SHOW_AFTER_MS = 150;

  let visible = $state(false);

  $effect(() => {
    if (!navigating.to) {
      visible = false;
      return;
    }
    const timer = setTimeout(() => (visible = true), SHOW_AFTER_MS);
    return () => clearTimeout(timer);
  });
</script>

{#if visible}
  <div
    class="pointer-events-none fixed inset-x-0 top-0 z-50 h-0.5 overflow-hidden"
    role="progressbar"
    aria-label="Loading page"
  >
    <div class="nav-progress bg-primary h-full w-1/3"></div>
  </div>
{/if}

<style>
  .nav-progress {
    animation: nav-progress 1s ease-in-out infinite;
  }

  @keyframes nav-progress {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
</style>
