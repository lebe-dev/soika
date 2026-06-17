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
    instance_role: 'member',
    notifications_enabled: true,
    auth_provider: 'local',
    ...overrides
  };
}

describe('authStore', () => {
  it('starts unknown: user is undefined; isAuthenticated, canManageInstance and isOwner are false', () => {
    expect(authStore.user).toBeUndefined();
    expect(authStore.isAuthenticated).toBe(false);
    expect(authStore.canManageInstance).toBe(false);
    expect(authStore.isOwner).toBe(false);
  });

  it('set() with a member is authenticated but cannot manage the instance', () => {
    const user = makeUser({ instance_role: 'member' });
    authStore.set(user);

    expect(authStore.user).toBe(user);
    expect(authStore.isAuthenticated).toBe(true);
    expect(authStore.canManageInstance).toBe(false);
    expect(authStore.isOwner).toBe(false);
  });

  it('set() with a manager reports canManageInstance true but isOwner false', () => {
    authStore.set(makeUser({ instance_role: 'manager' }));

    expect(authStore.isAuthenticated).toBe(true);
    expect(authStore.canManageInstance).toBe(true);
    expect(authStore.isOwner).toBe(false);
  });

  it('set() with an owner reports both canManageInstance and isOwner true', () => {
    authStore.set(makeUser({ instance_role: 'owner' }));

    expect(authStore.isAuthenticated).toBe(true);
    expect(authStore.canManageInstance).toBe(true);
    expect(authStore.isOwner).toBe(true);
  });

  it('clear() resets user to null and clears authentication', () => {
    authStore.set(makeUser({ instance_role: 'owner' }));
    authStore.clear();

    expect(authStore.user).toBeNull();
    expect(authStore.isAuthenticated).toBe(false);
    expect(authStore.canManageInstance).toBe(false);
    expect(authStore.isOwner).toBe(false);
  });
});
