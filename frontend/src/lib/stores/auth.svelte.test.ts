import { describe, expect, it } from 'vitest';

import type { User } from '$lib/api';

// `auth.svelte.ts` is a rune module (`$state`). This file is named
// `*.svelte.test.ts` so the svelte plugin compiles the runes (see vitest.config.ts).
import { authStore } from './auth.svelte';

function makeUser(overrides: Partial<User> = {}): User {
  return {
    id: 'u_123',
    email: 'user@example.com',
    display_name: 'Test User',
    is_admin: false,
    notifications_enabled: true,
    auth_provider: 'local',
    ...overrides
  };
}

describe('authStore', () => {
  it('starts unknown: user is undefined, isAuthenticated and isAdmin are false', () => {
    expect(authStore.user).toBeUndefined();
    expect(authStore.isAuthenticated).toBe(false);
    expect(authStore.isAdmin).toBe(false);
  });

  it('set() with a non-admin user is authenticated but not admin', () => {
    const user = makeUser({ is_admin: false });
    authStore.set(user);

    expect(authStore.user).toBe(user);
    expect(authStore.isAuthenticated).toBe(true);
    expect(authStore.isAdmin).toBe(false);
  });

  it('set() with an admin user reports isAdmin true', () => {
    authStore.set(makeUser({ is_admin: true }));

    expect(authStore.isAuthenticated).toBe(true);
    expect(authStore.isAdmin).toBe(true);
  });

  it('clear() resets user to null and clears authentication', () => {
    authStore.set(makeUser({ is_admin: true }));
    authStore.clear();

    expect(authStore.user).toBeNull();
    expect(authStore.isAuthenticated).toBe(false);
    expect(authStore.isAdmin).toBe(false);
  });
});
