// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { notifications } from '$lib/api';
import { setLocale } from '$lib/i18n';
import Control from './alertmanager-interval-control.svelte';
vi.mock('$app/navigation', () => ({ invalidateAll: vi.fn() }));
vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return { ...actual, notifications: { setInterval: vi.fn(), setProjectInterval: vi.fn() } };
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
function render(projectId?: string, readonly = false) {
  flushSync(() => {
    component = mount(Control, {
      target,
      props: {
        interval: { interval_seconds: projectId ? null : 300, effective_interval_seconds: 300 },
        projectId,
        readonly,
        managed: readonly
      }
    });
  });
}
it('saves a global cadence through the separate interval API', async () => {
  vi.mocked(notifications.setInterval).mockResolvedValue({
    interval_seconds: 120,
    effective_interval_seconds: 120
  });
  render();
  const input = target.querySelector<HTMLInputElement>('input[type="number"]')!;
  input.value = '120';
  flushSync(() => input.dispatchEvent(new Event('input', { bubbles: true })));
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() => expect(notifications.setInterval).toHaveBeenCalledWith(120));
});
it('sends null to remove the project override when inheritance is selected', async () => {
  vi.mocked(notifications.setProjectInterval).mockResolvedValue({
    interval_seconds: null,
    effective_interval_seconds: 300
  });
  render('project-id');
  expect(target.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
  expect(target.querySelector<HTMLInputElement>('input[type="number"]')!.disabled).toBe(true);
  target.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() =>
    expect(notifications.setProjectInterval).toHaveBeenCalledWith('project-id', null)
  );
});
it('disables interval controls when projects are managed through YAML', () => {
  render('project-id', true);
  expect(target.textContent).not.toContain('PROJECT_MANAGEMENT_SOURCE');
  expect(target.querySelector('button')).toBeNull();
  expect([...target.querySelectorAll('input')].every((input) => input.disabled)).toBe(true);
});
