// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import { api, ApiError, type NotificationView } from '$lib/api';
import Panel from './notifications-panel.svelte';
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
  feature_enabled: true,
  alertmanager_skip_tls: false,
  source: 'ui',
  variables: ['title'],
  config: {
    alertmanager: {
      enabled: false,
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
it('locks configuration in YAML mode while allowing a preview', () => {
  render({ ...view, source: 'yaml' });
  expect(target.textContent).not.toContain('Managed through YAML');
  expect(target.querySelector<HTMLInputElement>('#alertmanager-url')!.disabled).toBe(true);
  expect(target.querySelector<HTMLTextAreaElement>('#alertmanager-labels')!.disabled).toBe(true);
  expect(
    [...target.querySelectorAll('button')].some((b) => b.textContent?.includes('Save changes'))
  ).toBe(false);
});
it('localizes the form in Russian', () => {
  setLocale('ru');
  render();
  expect(target.textContent).toContain('Переменные шаблонов');
  expect(target.textContent).toContain('Базовый URL Alertmanager');
});
it('rejects invalid JSON before making an API call', async () => {
  render();
  const textarea = target.querySelector<HTMLTextAreaElement>('#alertmanager-labels')!;
  textarea.value = '{broken';
  flushSync(() => textarea.dispatchEvent(new Event('input', { bubbles: true })));
  const save = [...target.querySelectorAll('button')].find((b) =>
    b.textContent?.includes('Save changes')
  )!;
  save.click();
  await vi.waitFor(() =>
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('JSON object')
  );
  expect(api.notifications.update).not.toHaveBeenCalled();
});

it('shows the environment default without persisting an override', () => {
  render({
    ...view,
    default_url: 'http://alertmanager:9093',
    config: { ...view.config, alertmanager: { ...view.config.alertmanager, url: '' } }
  });
  const input = target.querySelector<HTMLInputElement>('#alertmanager-url')!;
  expect(input.value).toBe('');
  expect(input.placeholder).toBe('http://alertmanager:9093');
  expect(target.textContent).toContain('Leave the URL empty to use ALERTMANAGER_URL');
});

it.each([
  ['en', 'Disabled'],
  ['ru', 'Отключено']
] as const)(
  'shows a concise disabled state in %s when the feature flag is off',
  (language, message) => {
    setLocale(language);
    render({ ...view, feature_enabled: false });
    expect(target.textContent).toContain(message);
    expect(target.textContent).not.toContain('NOTIFICATIONS_ALERTMANAGER_ENABLED');
    expect(target.querySelector('input, textarea, button')).toBeNull();
    expect(api.notifications.update).not.toHaveBeenCalled();
    expect(api.notifications.preview).not.toHaveBeenCalled();
  }
);

it.each(['en', 'ru'] as const)('sends a test in %s using saved settings', async (language) => {
  setLocale(language);
  vi.mocked(api.notifications.test).mockResolvedValue({ ok: true });
  render({
    ...view,
    config: { ...view.config, alertmanager: { ...view.config.alertmanager, enabled: true } }
  });
  const button = [...target.querySelectorAll('button')].find((b) =>
    b.textContent?.includes(language === 'ru' ? 'Отправить тестовый' : 'Send test alert')
  )!;
  expect(button.disabled).toBe(false);
  button.click();
  await vi.waitFor(() => expect(api.notifications.test).toHaveBeenCalledWith(language));
});
it('allows a test when the channel is disabled', () => {
  render();
  expect(
    [...target.querySelectorAll('button')].find((b) => b.textContent?.includes('Send test alert'))
      ?.disabled
  ).toBe(false);
});
it('allows testing with unsaved settings and displays delivery failures', async () => {
  render({
    ...view,
    config: { ...view.config, alertmanager: { ...view.config.alertmanager, enabled: true } }
  });
  const button = [...target.querySelectorAll('button')].find((b) =>
    b.textContent?.includes('Send test alert')
  )!;
  const input = target.querySelector<HTMLTextAreaElement>('#alertmanager-labels')!;
  input.value = '{"alertname":"Changed"}';
  flushSync(() => input.dispatchEvent(new Event('input', { bubbles: true })));
  expect(button.disabled).toBe(false);
  vi.mocked(api.notifications.test).mockRejectedValue(
    new ApiError(500, 'Alertmanager delivery failed: connection refused', null)
  );
  button.click();
  await vi.waitFor(() =>
    expect(target.querySelector('[role="alert"]')?.textContent).toContain(
      'Alertmanager delivery failed: connection refused'
    )
  );
  await vi.waitFor(() => expect(button.disabled).toBe(false));
});

it('requires a saved URL in UI mode even when an environment fallback exists', () => {
  render({
    ...view,
    default_url: 'http://fallback:9093',
    config: { ...view.config, alertmanager: { ...view.config.alertmanager, url: '' } }
  });
  const button = [...target.querySelectorAll('button')].find((b) =>
    b.textContent?.includes('Send test alert')
  )!;
  expect(button.disabled).toBe(true);
  const input = target.querySelector<HTMLInputElement>('#alertmanager-url')!;
  input.value = 'http://new:9093';
  flushSync(() => input.dispatchEvent(new Event('input', { bubbles: true })));
  expect(button.disabled).toBe(true);
});

it('allows the test action in YAML mode', () => {
  render({
    ...view,
    source: 'yaml',
    config: { ...view.config, alertmanager: { ...view.config.alertmanager, enabled: true } }
  });
  const button = [...target.querySelectorAll('button')].find((b) =>
    b.textContent?.includes('Send test alert')
  )!;
  expect(button.disabled).toBe(false);
  expect(target.textContent).not.toContain('Managed through YAML');
});

it('locks notification settings while allowing testing in env mode', () => {
  render({
    ...view,
    source: 'env',
    config: { ...view.config, alertmanager: { ...view.config.alertmanager, enabled: true } }
  });
  expect(target.textContent).not.toContain('environment variables');
  expect(target.querySelector<HTMLInputElement>('#alertmanager-url')!.disabled).toBe(true);
  expect(
    [...target.querySelectorAll('button')].find((b) => b.textContent?.includes('Send test alert'))!
      .disabled
  ).toBe(false);
  expect(target.textContent).not.toContain('Save changes');
});

it.each(['en', 'ru'] as const)('warns about disabled TLS verification in %s', (language) => {
  setLocale(language);
  render({ ...view, alertmanager_skip_tls: true });
  expect(target.querySelector('[role="status"]')?.textContent).toContain(
    'ALERTMANAGER_SKIP_TLS=true'
  );
});
it('hides the TLS warning when verification is enabled', () => {
  render();
  expect(target.textContent).not.toContain('ALERTMANAGER_SKIP_TLS=true');
});
