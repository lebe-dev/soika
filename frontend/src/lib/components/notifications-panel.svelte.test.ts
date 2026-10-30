// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import { api, type NotificationView } from '$lib/api';
import Panel from './notifications-panel.svelte';
vi.mock('$app/navigation', () => ({ invalidateAll: vi.fn() }));
vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    api: { ...actual.api, notifications: { update: vi.fn(), preview: vi.fn() } }
  };
});
vi.mock('$lib/report', () => ({ reportUnexpected: vi.fn() }));
vi.mock('$lib/components/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
let component: ReturnType<typeof mount>;
let target: HTMLDivElement;
const view: NotificationView = {
  feature_enabled: true,
  source: 'ui',
  variables: ['title'],
  config: {
    alertmanager: {
      enabled: false,
      url: '',
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
  expect(target.textContent).toContain('Restart Soika');
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
  render({ ...view, default_url: 'http://alertmanager:9093' });
  const input = target.querySelector<HTMLInputElement>('#alertmanager-url')!;
  expect(input.value).toBe('');
  expect(input.placeholder).toBe('http://alertmanager:9093');
  expect(target.textContent).toContain('Leave the URL empty to use ALERTMANAGER_URL');
});

it.each([
  ['en', 'Disabled in configuration'],
  ['ru', 'Отключено в конфигурации']
] as const)(
  'shows a concise disabled state in %s when the feature flag is off',
  (language, message) => {
    setLocale(language);
    render({ ...view, feature_enabled: false });
    expect(target.textContent).toContain(message);
    expect(target.textContent).toContain('NOTIFICATIONS_ALERTMANAGER_ENABLED=false');
    expect(target.querySelector('input, textarea, button')).toBeNull();
    expect(api.notifications.update).not.toHaveBeenCalled();
    expect(api.notifications.preview).not.toHaveBeenCalled();
  }
);
