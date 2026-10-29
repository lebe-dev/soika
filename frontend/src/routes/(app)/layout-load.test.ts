import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    api: {
      ...actual.api,
      admin: { users: vi.fn() }
    }
  };
});

vi.mock('@sveltejs/kit', async () => {
  const actual = await vi.importActual<typeof import('@sveltejs/kit')>('@sveltejs/kit');
  return {
    ...actual,
    redirect: (status: number, location: string) => {
      throw { status, location };
    }
  };
});

import { api } from '$lib/api';
import { load } from './+layout';

const adminUsers = vi.mocked(api.admin.users);

// How many times the load read `url` outside of `untrack`. A tracked read
// reruns the load — and its user-list request — on every navigation.
let trackedUrlReads = 0;

function callLoad(user: unknown, users: unknown[] = []) {
  const url = new URL('https://app.test/');
  let untracked = false;
  return (load as (e: unknown) => Promise<unknown>)({
    parent: async () => ({ user, config: { admin: { users } } }),
    // Like SvelteKit, a read of any URL property is what registers the
    // dependency, so the proxy counts reads made outside `untrack`.
    url: new Proxy(url, {
      get(target, prop) {
        if (!untracked) trackedUrlReads += 1;
        return Reflect.get(target, prop, target);
      }
    }),
    untrack<T>(fn: () => T): T {
      untracked = true;
      try {
        return fn();
      } finally {
        untracked = false;
      }
    },
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  trackedUrlReads = 0;
});

describe('(app) +layout load', () => {
  it('redirects to /login when there is no authenticated user', async () => {
    await expect(callLoad(null)).rejects.toMatchObject({ status: 307 });
    expect(adminUsers).not.toHaveBeenCalled();
  });

  it('skips the user list and returns pendingApprovals=0 for a plain member', async () => {
    const data = (await callLoad({ id: 'u1', instance_role: 'member' })) as {
      user: { id: string };
      pendingApprovals: number;
    };

    expect(adminUsers).not.toHaveBeenCalled();
    expect(data.pendingApprovals).toBe(0);
    expect(data.user.id).toBe('u1');
  });

  it('counts pending accounts for a manager', async () => {
    adminUsers.mockResolvedValue([
      { id: 'u1', status: 'pending' },
      { id: 'u2', status: 'active' },
      { id: 'u3', status: 'pending' }
    ] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'manager' }, [
      { status: 'pending' },
      { status: 'active' },
      { status: 'pending' }
    ])) as {
      pendingApprovals: number;
    };

    expect(adminUsers).not.toHaveBeenCalled();
    expect(data.pendingApprovals).toBe(2);
  });

  it('counts pending accounts for an owner too', async () => {
    adminUsers.mockResolvedValue([{ id: 'u1', status: 'pending' }] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'owner' }, [
      { status: 'pending' }
    ])) as {
      pendingApprovals: number;
    };

    expect(adminUsers).not.toHaveBeenCalled();
    expect(data.pendingApprovals).toBe(1);
  });

  it('does not rerun on navigation: the url is read untracked', async () => {
    adminUsers.mockResolvedValue([] as never);

    await callLoad({ id: 'u1', instance_role: 'owner' });

    expect(trackedUrlReads).toBe(0);
  });

  it('falls back to pendingApprovals=0 when the user list fetch fails', async () => {
    adminUsers.mockRejectedValue(new Error('boom'));

    const data = (await callLoad({ id: 'u1', instance_role: 'owner' })) as {
      pendingApprovals: number;
    };

    expect(data.pendingApprovals).toBe(0);
  });
});
