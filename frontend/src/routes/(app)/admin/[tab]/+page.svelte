<script lang="ts">
  import AdminPage from '../+page.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();
</script>

<AdminPage {data} />
