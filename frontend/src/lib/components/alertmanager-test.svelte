<script lang="ts">
  import { api, errorMessage, type NotificationView } from '$lib/api';
  import { t, currentLocale } from '$lib/i18n';
  import { reportUnexpected } from '$lib/report';
  import * as Card from '$lib/components/ui/card';
  import { Button } from '$lib/components/ui/button';
  import { toast } from '$lib/components/ui/sonner';

  let { view }: { view: NotificationView } = $props();
  let sending = $state(false);
  let error = $state('');
  const enabled = $derived(view.feature_enabled && view.config.alertmanager.enabled);
  const ready = $derived(view.source !== 'ui' || Boolean(view.config.alertmanager.url.trim()));

  async function sendTest() {
    if (sending || !enabled || !ready) return;
    sending = true;
    error = '';
    try {
      await api.notifications.test(currentLocale());
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
  <Card.Root>
    <Card.Header>
      <Card.Title>Alertmanager</Card.Title>
      <Card.Description>{$t('notifications_test_help')}</Card.Description>
    </Card.Header>
    <Card.Content class="space-y-3">
      <Button variant="outline" onclick={sendTest} disabled={sending || !ready}>
        {sending ? $t('notifications_test_sending') : $t('notifications_test')}
      </Button>
      {#if error}<p role="alert" class="text-destructive text-sm">{error}</p>{/if}
    </Card.Content>
  </Card.Root>
{/if}
