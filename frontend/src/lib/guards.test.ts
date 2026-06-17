import { describe, expect, it, vi } from 'vitest';
import type { User } from '$lib/api';
import { requireInstanceManager, requireUser } from './guards';

// `redirect()` from SvelteKit throws a control-flow object to unwind the load
// function. Mock it as a thrower carrying the status + location so tests can
// assert what the guard tried to redirect to.
class MockRedirect extends Error {
  constructor(
    readonly status: number,
    readonly location: string
  ) {
    super(`redirect ${status} -> ${location}`);
    this.name = 'MockRedirect';
  }
}

vi.mock('@sveltejs/kit', () => ({
  redirect: (status: number, location: string) => {
    throw new MockRedirect(status, location);
  }
}));

function makeUser(overrides: Partial<User> = {}): User {
  return {
    id: 'user-1',
    email: 'user@example.com',
    display_name: 'User',
    instance_role: 'member',
    notifications_enabled: true,
    auth_provider: 'local',
    ...overrides
  };
}

describe('requireUser', () => {
  it('returns the user when a session is present', () => {
    const user = makeUser();
    expect(requireUser(user, '/projects')).toBe(user);
  });

  it('redirects (307) to /login preserving the target path', () => {
    let thrown: unknown;
    try {
      requireUser(null, '/projects/42');
    } catch (e) {
      thrown = e;
    }
    expect(thrown).toBeInstanceOf(MockRedirect);
    const redirect = thrown as MockRedirect;
    expect(redirect.status).toBe(307);
    expect(redirect.location).toBe('/login?next=%2Fprojects%2F42');
  });

  it('percent-encodes a target path containing ?, & and a space', () => {
    let thrown: unknown;
    try {
      requireUser(undefined, '/search?q=a b&sort=new');
    } catch (e) {
      thrown = e;
    }
    expect(thrown).toBeInstanceOf(MockRedirect);
    const redirect = thrown as MockRedirect;
    expect(redirect.status).toBe(307);
    // encodeURIComponent: '/'→%2F, '?'→%3F, ' '→%20, '&'→%26, '='→%3D
    expect(redirect.location).toBe('/login?next=%2Fsearch%3Fq%3Da%20b%26sort%3Dnew');
  });
});

describe('requireInstanceManager', () => {
  it('returns the user when they are an owner', () => {
    const owner = makeUser({ instance_role: 'owner' });
    expect(requireInstanceManager(owner, '/admin')).toBe(owner);
  });

  it('returns the user when they are a manager', () => {
    const manager = makeUser({ instance_role: 'manager' });
    expect(requireInstanceManager(manager, '/admin')).toBe(manager);
  });

  it('redirects (307) to / for a signed-in member', () => {
    let thrown: unknown;
    try {
      requireInstanceManager(makeUser({ instance_role: 'member' }), '/admin');
    } catch (e) {
      thrown = e;
    }
    expect(thrown).toBeInstanceOf(MockRedirect);
    const redirect = thrown as MockRedirect;
    expect(redirect.status).toBe(307);
    expect(redirect.location).toBe('/');
  });

  it('delegates to requireUser (redirects to /login) for an anonymous caller', () => {
    let thrown: unknown;
    try {
      requireInstanceManager(null, '/admin/users');
    } catch (e) {
      thrown = e;
    }
    expect(thrown).toBeInstanceOf(MockRedirect);
    const redirect = thrown as MockRedirect;
    expect(redirect.status).toBe(307);
    expect(redirect.location).toBe('/login?next=%2Fadmin%2Fusers');
  });
});
