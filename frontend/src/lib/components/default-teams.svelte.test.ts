// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import DefaultTeams from './default-teams.svelte';

const { update, invalidateAll, success, failure } = vi.hoisted(() => ({
  update: vi.fn(),
  invalidateAll: vi.fn(),
  success: vi.fn(),
  failure: vi.fn()
}));
vi.mock('$app/navigation', () => ({ invalidateAll }));
vi.mock('$lib/api', () => ({
  api: { settings: { update } },
  errorMessage: (_err: unknown, fallback: string) => fallback
}));
vi.mock('$lib/report', () => ({ reportUnexpected: vi.fn() }));
vi.mock('$lib/components/ui/sonner', () => ({ toast: { success, error: failure } }));

const teams = ['A', 'B'].map((name) => ({ id: name, name, created_at: '', updated_at: '' }));
let component: ReturnType<typeof mount>;
let target: HTMLDivElement;
function render(defaultTeamIds: string[] = [], availableTeams = teams) {
  flushSync(() => {
    component = mount(DefaultTeams, { target, props: { teams: availableTeams, defaultTeamIds } });
  });
}
function checkboxes() {
  return [...target.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
}
function button(label: string) {
  return [...target.querySelectorAll<HTMLButtonElement>('button')].find(
    (b) => b.textContent?.trim() === label
  )!;
}
function toggle(index: number) {
  checkboxes()[index].click();
  flushSync();
}

beforeEach(() => {
  vi.clearAllMocks();
  invalidateAll.mockResolvedValue(undefined);
  setLocale('en');
  target = document.createElement('div');
  document.body.appendChild(target);
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  setLocale('en');
});

describe('default teams for new users', () => {
  it('starts unselected and explains that existing users are unaffected', () => {
    render();
    expect(checkboxes().map((c) => c.checked)).toEqual([false, false]);
    expect(button('Save changes').disabled).toBe(true);
    expect(target.textContent).toContain('Existing users and their memberships are unchanged');
    expect(update).not.toHaveBeenCalled();
  });
  it('shows saved selections and resets unsaved changes without making a request', () => {
    render(['B']);
    expect(checkboxes().map((c) => c.checked)).toEqual([false, true]);
    toggle(0);
    toggle(1);
    expect(button('Save changes').disabled).toBe(false);
    button('Reset').click();
    flushSync();
    expect(checkboxes().map((c) => c.checked)).toEqual([false, true]);
    expect(button('Save changes').disabled).toBe(true);
    expect(update).not.toHaveBeenCalled();
  });
  it('saves multiple teams explicitly and disables controls while saving', async () => {
    let finish!: (value: unknown) => void;
    update.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    render();
    toggle(0);
    toggle(1);
    button('Save changes').click();
    flushSync();
    expect(update).toHaveBeenCalledExactlyOnceWith({ default_team_ids: ['A', 'B'] });
    expect(checkboxes().every((c) => c.matches(':disabled'))).toBe(true);
    expect(button('Reset').disabled).toBe(true);
    finish({ default_team_ids: ['A', 'B'] });
    await vi.waitFor(() => expect(success).toHaveBeenCalled());
    expect(invalidateAll).toHaveBeenCalledOnce();
    expect(button('Save changes').disabled).toBe(true);
    expect(checkboxes().map((c) => c.checked)).toEqual([true, true]);
  });
  it('saves an empty list when all defaults are cleared', async () => {
    update.mockResolvedValue({ default_team_ids: [] });
    render(['A']);
    toggle(0);
    button('Save changes').click();
    await vi.waitFor(() => expect(success).toHaveBeenCalled());
    expect(update).toHaveBeenCalledExactlyOnceWith({ default_team_ids: [] });
    expect(checkboxes().every((c) => !c.checked)).toBe(true);
  });
  it('preserves the draft after failure and allows retry or reset', async () => {
    update.mockRejectedValue(new Error('unavailable'));
    render(['A']);
    toggle(1);
    button('Save changes').click();
    await vi.waitFor(() => expect(failure).toHaveBeenCalled());
    expect(button('Save changes').disabled).toBe(false);
    expect(checkboxes().map((c) => c.checked)).toEqual([true, true]);
    expect(invalidateAll).not.toHaveBeenCalled();
    button('Reset').click();
    flushSync();
    expect(checkboxes().map((c) => c.checked)).toEqual([true, false]);
  });
  it('handles an instance with no teams', () => {
    render([], []);
    expect(checkboxes()).toHaveLength(0);
    expect(target.textContent).toContain('Create a team first');
    expect(button('Save changes').disabled).toBe(true);
  });
  it('provides Russian labels and explanation', () => {
    setLocale('ru');
    render();
    expect(target.textContent).toContain('Команды для новых пользователей');
    expect(target.textContent).toContain('Существующие пользователи и их членство не изменяются');
  });
});
