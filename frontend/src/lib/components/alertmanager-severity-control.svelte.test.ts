// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { notifications } from '$lib/api';
import { setLocale } from '$lib/i18n';
import Control from './alertmanager-severity-control.svelte';
vi.mock('$app/navigation', () => ({ invalidateAll: vi.fn() }));
vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return { ...actual, notifications: { setProjectSeverity: vi.fn() } };
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
function render(readonly = false) {
  flushSync(() => {
    component = mount(Control, {
      target,
      props: { view: { severity: 'warning' }, projectId: 'api', readonly, managed: readonly }
    });
  });
}
it('saves the selected project alert class', async () => {
  vi.mocked(notifications.setProjectSeverity).mockResolvedValue({ severity: 'critical' });
  render();
  const select = target.querySelector('select')!;
  select.value = 'critical';
  flushSync(() => select.dispatchEvent(new Event('change', { bubbles: true })));
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() =>
    expect(notifications.setProjectSeverity).toHaveBeenCalledWith('api', 'critical')
  );
});
it('clears the project override to use the existing notification template', async () => {
  vi.mocked(notifications.setProjectSeverity).mockResolvedValue({ severity: null });
  render();
  const select = target.querySelector('select')!;
  select.value = '';
  flushSync(() => select.dispatchEvent(new Event('change', { bubbles: true })));
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() =>
    expect(notifications.setProjectSeverity).toHaveBeenCalledWith('api', null)
  );
});
it('locks the selector for YAML-managed projects', () => {
  render(true);
  expect(target.querySelector('select')!.disabled).toBe(true);
  expect(target.querySelector('button')).toBeNull();
  expect(target.textContent).toContain('PROJECT_MANAGEMENT_SOURCE');
});
