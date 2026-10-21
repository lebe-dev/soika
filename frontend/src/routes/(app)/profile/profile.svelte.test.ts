// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { User } from '$lib/api';
import { authStore } from '$lib/stores/auth.svelte';
import { currentLocale, setLocale } from '$lib/i18n';
import ProfileLayout from '../../../test/profile-layout.svelte';

const { update, success, failure } = vi.hoisted(() => ({
  update: vi.fn(),
  success: vi.fn(),
  failure: vi.fn()
}));
vi.mock('$app/navigation', () => ({ invalidateAll: vi.fn().mockResolvedValue(undefined) }));
vi.mock('$app/stores', async () => {
  const { readable } = await import('svelte/store');
  return { page: readable({ data: { config: { passkey_enabled: false }, telemetry: null } }) };
});
vi.mock('$lib/api', () => ({
  profile: { update },
  passkeys: {},
  errorMessage: (_error: unknown, fallback: string) => fallback
}));
vi.mock('$lib/report', () => ({ reportUnexpected: vi.fn() }));
vi.mock('$lib/passkey', () => ({
  passkeysSupported: () => false,
  registerPasskey: vi.fn(),
  isCeremonyCancelled: () => false
}));
vi.mock('$lib/components/ui/sonner', () => ({ toast: { success, error: failure } }));

const user: User = {
  id: 'u1',
  email: 'member@example.com',
  display_name: 'Member',
  instance_role: 'member',
  notifications_enabled: true,
  auth_provider: 'local',
  locale: 'en'
};
let component: ReturnType<typeof mount>;
let target: HTMLDivElement;
beforeEach(() => {
  vi.clearAllMocks();
  authStore.set({ ...user });
  setLocale('en');
  target = document.createElement('div');
  document.body.appendChild(target);
  flushSync(() => {
    component = mount(ProfileLayout, { target, props: { user } });
  });
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  setLocale('en');
});

describe('profile language selector', () => {
  it('saves the choice, translates the mounted UI, and updates document language', async () => {
    let finish!: (value: User) => void;
    update.mockImplementation(
      () =>
        new Promise<User>((resolve) => {
          finish = resolve;
        })
    );
    expect(target.textContent).toContain('Interface language');
    const select = target.querySelector<HTMLSelectElement>('#language')!;
    select.value = 'ru';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    expect(update).toHaveBeenCalledWith({ locale: 'ru' });
    expect(currentLocale()).toBe('en');
    expect(select.disabled).toBe(true);
    finish({ ...user, locale: 'ru' });
    await vi.waitFor(() => expect(target.textContent).toContain('Язык интерфейса'));
    expect(authStore.user?.locale).toBe('ru');
    expect(document.documentElement.lang).toBe('ru');
    expect(select.value).toBe('ru');
    expect(select.disabled).toBe(false);
    expect(success).toHaveBeenCalledWith('Язык обновлён');
  });

  it('restores the saved selection and shows an error when saving fails', async () => {
    update.mockRejectedValue(new Error('write failed'));
    const select = target.querySelector<HTMLSelectElement>('#language')!;
    select.value = 'ru';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await vi.waitFor(() => expect(failure).toHaveBeenCalled());
    expect(currentLocale()).toBe('en');
    expect(authStore.user?.locale).toBe('en');
    expect(select.value).toBe('en');
    expect(select.disabled).toBe(false);
  });
});
