// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import { api, ApiError } from '$lib/api';
import Panel from './alertmanager-test.svelte';
vi.mock('$app/navigation', () => ({ invalidateAll: vi.fn() }));
vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    api: {
      ...actual.api,
      notifications: { update: vi.fn(), preview: vi.fn(), projectTest: vi.fn() }
    }
  };
});
vi.mock('$lib/report', () => ({ reportUnexpected: vi.fn() }));
vi.mock('$lib/components/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
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
  setLocale('en');
});
function render(enabled = true) {
  flushSync(() => {
    component = mount(Panel, { target, props: { projectId: 'project-1', enabled } });
  });
}
it('hides the action when integration is disabled or caller cannot administer the project', () => {
  render(false);
  expect(target.querySelector('button')).toBeNull();
});

it.each(['en', 'ru'] as const)(
  'sends a test in %s and prevents duplicate sends',
  async (language) => {
    setLocale(language);
    let finish!: (value: { ok: boolean }) => void;
    vi.mocked(api.notifications.projectTest).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    render();
    const button = target.querySelector('button')!;
    flushSync(() => button.click());
    expect(button.disabled).toBe(true);
    button.click();
    expect(api.notifications.projectTest).toHaveBeenCalledExactlyOnceWith('project-1', language);
    finish({ ok: true });
    await vi.waitFor(() => expect(button.disabled).toBe(false));
  }
);

it('shows delivery failures and allows retrying', async () => {
  vi.mocked(api.notifications.projectTest).mockRejectedValue(
    new ApiError(500, 'Delivery failed', null)
  );
  render();
  const button = target.querySelector('button')!;
  button.click();
  await vi.waitFor(() =>
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('Delivery failed')
  );
  expect(button.disabled).toBe(false);
  vi.mocked(api.notifications.projectTest).mockResolvedValue({ ok: true });
  button.click();
  await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeNull());
});
