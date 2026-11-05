<script lang="ts">
  import { api, errorMessage } from '$lib/api';
  import { t, currentLocale } from '$lib/i18n';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { toast } from '$lib/components/ui/sonner';

  let { projectId, enabled }: { projectId: string; enabled: boolean } = $props();
  let sending = $state(false);
  let error = $state('');

  async function sendTest() {
    if (sending || !enabled) return;
    sending = true;
    error = '';
    try {
      await api.notifications.projectTest(projectId, currentLocale());
      toast.success($t('notifications_test_sent'));
    } catch (err) {
      error = errorMessage(err, $t('notifications_test_failed'));
      reportUnexpected(err);
    } finally {
      sending = false;
    }
  }
</script>

{#if enabled}
  <Card.Root class="contents">
    <Card.Header>
      <Card.Title>Alertmanager</Card.Title>
      <Card.Description>{$t('notifications_project_test_help')}</Card.Description>
    </Card.Header>
    <Card.Content class="space-y-3">
      <Button variant="outline" onclick={sendTest} disabled={sending}>
        {sending ? $t('notifications_test_sending') : $t('notifications_test')}
      </Button>
      {#if error}<p role="alert" class="text-destructive text-sm">{error}</p>{/if}
    </Card.Content>
  </Card.Root>
{/if}
