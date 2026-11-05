// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import { api, ApiError, type NotificationView } from '$lib/api';
import Panel from './alertmanager-test.svelte';
vi.mock('$app/navigation', () => ({ invalidateAll: vi.fn() }));
vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    api: { ...actual.api, notifications: { update: vi.fn(), preview: vi.fn(), test: vi.fn() } }
  };
});
vi.mock('$lib/report', () => ({ reportUnexpected: vi.fn() }));
vi.mock('$lib/components/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
let component: ReturnType<typeof mount>;
let target: HTMLDivElement;
const view: NotificationView = {
  interval_source: 'ui',
  default_severity: null,
  interval: { interval_seconds: 60, effective_interval_seconds: 60 },
  feature_enabled: true,
  alertmanager_skip_tls: false,
  source: 'ui',
  variables: ['title'],
  config: {
    alertmanager: {
      enabled: true,
      url: 'http://alertmanager:9093',
      labels: { alertname: 'SoikaIssue' },
      annotations: { summary: '{{ title }}' }
    }
  }
};
beforeEach(() => {
  vi.clearAllMocks();
  setLocale('en');
  target = document.createElement('div');
  document.body.appendChild(target);
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  setLocale('en');
});
function render(value = view) {
  flushSync(() => {
    component = mount(Panel, { target, props: { view: structuredClone(value) } });
  });
}
it.each([
  { ...view, feature_enabled: false },
  { ...view, config: { alertmanager: { ...view.config.alertmanager, enabled: false } } }
])('hides the action when the integration is disabled', (value) => {
  render(value);
  expect(target.querySelector('button')).toBeNull();
});

it.each(['en', 'ru'] as const)(
  'sends a test in %s and prevents duplicate sends',
  async (language) => {
    setLocale(language);
    let finish!: (value: { ok: boolean }) => void;
    vi.mocked(api.notifications.test).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    render();
    const button = target.querySelector('button')!;
    flushSync(() => button.click());
    expect(button.disabled).toBe(true);
    button.click();
    expect(api.notifications.test).toHaveBeenCalledExactlyOnceWith(language);
    finish({ ok: true });
    await vi.waitFor(() => expect(button.disabled).toBe(false));
  }
);

it.each(['yaml', 'env'] as const)('allows testing with %s managed settings', (source) => {
  render({ ...view, source });
  expect(target.querySelector('button')!.disabled).toBe(false);
});

it('requires a saved URL in UI mode', () => {
  render({ ...view, config: { alertmanager: { ...view.config.alertmanager, url: '' } } });
  expect(target.querySelector('button')!.disabled).toBe(true);
});

it('shows delivery failures and allows retrying', async () => {
  vi.mocked(api.notifications.test).mockRejectedValue(new ApiError(500, 'Delivery failed', null));
  render();
  const button = target.querySelector('button')!;
  button.click();
  await vi.waitFor(() =>
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('Delivery failed')
  );
  expect(button.disabled).toBe(false);
  vi.mocked(api.notifications.test).mockResolvedValue({ ok: true });
  button.click();
  await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeNull());
});
