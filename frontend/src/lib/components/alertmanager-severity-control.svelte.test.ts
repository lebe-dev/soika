// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { notifications } from '$lib/api';
import { setLocale } from '$lib/i18n';
import Control from './alertmanager-severity-control.svelte';
vi.mock('$app/navigation', () => ({ invalidateAll: vi.fn() }));
vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return { ...actual, notifications: { setProjectSeverity: vi.fn(), setDefaultSeverity: vi.fn() } };
});
vi.mock('$lib/report', () => ({ reportUnexpected: vi.fn() }));
vi.mock('$lib/components/ui/sonner', () => ({ toast: { success: vi.fn() } }));
let component: ReturnType<typeof mount>;
let target: HTMLDivElement;
beforeEach(() => {
  vi.clearAllMocks();
  setLocale('en');
  target = document.createElement('div');
  document.body.appendChild(target);
});
afterEach(async () => {
  await unmount(component);
  target.remove();
});
function render(readonly = false, global = false) {
  flushSync(() => {
    component = mount(Control, {
      target,
      props: {
        view: { severity: 'warning' },
        projectId: global ? undefined : 'api',
        readonly,
        managed: readonly
      }
    });
  });
}
it('saves the selected project alert class', async () => {
  vi.mocked(notifications.setProjectSeverity).mockResolvedValue({ severity: 'critical' });
  render();
  const select = target.querySelectorAll('select')[1]!;
  select.value = 'critical';
  flushSync(() => select.dispatchEvent(new Event('change', { bubbles: true })));
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() =>
    expect(notifications.setProjectSeverity).toHaveBeenCalledWith('api', 'critical', 'fixed')
  );
});
it('clears the project override to inherit the global default', async () => {
  vi.mocked(notifications.setProjectSeverity).mockResolvedValue({ severity: null });
  render();
  const select = target.querySelectorAll('select')[1]!;
  const mode = target.querySelector('select')!;
  mode.value = '';
  flushSync(() => mode.dispatchEvent(new Event('change', { bubbles: true })));
  select.value = '';
  flushSync(() => select.dispatchEvent(new Event('change', { bubbles: true })));
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() =>
    expect(notifications.setProjectSeverity).toHaveBeenCalledWith('api', null, null)
  );
});
it('locks the selector for YAML-managed projects', () => {
  render(true);
  expect(target.querySelectorAll('select')[1]!.disabled).toBe(true);
  expect(target.querySelector('button')).toBeNull();
  expect(target.textContent).not.toContain('PROJECT_MANAGEMENT_SOURCE');
});

it('saves an instance default when no project is selected', async () => {
  vi.mocked(notifications.setDefaultSeverity).mockResolvedValue({ severity: 'info' });
  render(false, true);
  const select = target.querySelectorAll('select')[1]!;
  select.value = 'info';
  flushSync(() => select.dispatchEvent(new Event('change', { bubbles: true })));
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() =>
    expect(notifications.setDefaultSeverity).toHaveBeenCalledWith('info', 'fixed')
  );
});
it('shows environment ownership for the instance default', () => {
  render(true, true);
  expect(target.textContent).not.toContain('SERVICE_MANAGEMENT_SOURCE');
  expect(target.querySelector('select')!.hasAttribute('data-managed-readonly')).toBe(true);
  expect(target.querySelectorAll('select')[1]!.disabled).toBe(true);
});

it('saves automatic mode with its fallback and explains the mapping', async () => {
  vi.mocked(notifications.setProjectSeverity).mockResolvedValue({
    severity: 'warning',
    mode: 'event'
  });
  render();
  const mode = target.querySelector('select')!;
  mode.value = 'event';
  flushSync(() => mode.dispatchEvent(new Event('change', { bubbles: true })));
  expect(target.textContent).toContain('fatal / error → critical');
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() =>
    expect(notifications.setProjectSeverity).toHaveBeenCalledWith('api', 'warning', 'event')
  );
});
