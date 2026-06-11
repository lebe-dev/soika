<script lang="ts">
  // App-wide error boundary. SvelteKit renders this for thrown `error(...)`
  // results (404/403/…) and for uncaught load/render exceptions, replacing its
  // unstyled default page with an in-app card matching the auth screens.
  import { page } from '$app/state';
  import { Button } from '$lib/components/ui/button';
  import * as Card from '$lib/components/ui/card';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import PageTitle from '$lib/components/page-title.svelte';

  const status = $derived(page.status);
  const message = $derived(page.error?.message ?? 'Something went wrong.');
</script>

<PageTitle title={`Error ${status}`} />

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-sm">
    <Card.Header>
      <div
        class="bg-muted text-muted-foreground mb-2 flex size-10 items-center justify-center rounded-full"
      >
        <TriangleAlert class="size-5" />
      </div>
      <Card.Title>Error {status}</Card.Title>
      <Card.Description>{message}</Card.Description>
    </Card.Header>
    <Card.Footer>
      <Button href="/" class="w-full">Back to dashboard</Button>
    </Card.Footer>
  </Card.Root>
</div>
