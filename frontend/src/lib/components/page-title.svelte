<script lang="ts" module>
  // Suffix appended to every page title so browser tabs/bookmarks stay branded.
  // SSR is disabled (SPA), so titles are set on the client via <svelte:head>.
  const SUFFIX = 'soika';
</script>

<script lang="ts">
  // Renders the document <title> for a route. Pass the page-specific part; the
  // brand suffix is added automatically. Omit `title` for the brand alone.
  let { title }: { title?: string } = $props();

  const full = $derived(title?.trim() ? `${title.trim()} · ${SUFFIX}` : SUFFIX);
</script>

<svelte:head>
  <title>{full}</title>
</svelte:head>
