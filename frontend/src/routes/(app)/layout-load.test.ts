// Tests for the authenticated app-shell layout load: it requires a session
// (redirect to /login otherwise) and, only for instance managers (Owner |
// Manager), fetches the user list to surface the count of accounts awaiting
// approval on the Admin nav item. A failure of that supplementary call must
// not break the shell — it falls back to 0.

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

function callLoad(user: unknown) {
  return (load as (e: unknown) => Promise<unknown>)({
    parent: async () => ({ user }),
    url: new URL('https://app.test/'),
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
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

    const data = (await callLoad({ id: 'u1', instance_role: 'manager' })) as {
      pendingApprovals: number;
    };

    expect(adminUsers).toHaveBeenCalledTimes(1);
    expect(data.pendingApprovals).toBe(2);
  });

  it('counts pending accounts for an owner too', async () => {
    adminUsers.mockResolvedValue([{ id: 'u1', status: 'pending' }] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'owner' })) as {
      pendingApprovals: number;
    };

    expect(adminUsers).toHaveBeenCalledTimes(1);
    expect(data.pendingApprovals).toBe(1);
  });

  it('falls back to pendingApprovals=0 when the user list fetch fails', async () => {
    adminUsers.mockRejectedValue(new Error('boom'));

    const data = (await callLoad({ id: 'u1', instance_role: 'owner' })) as {
      pendingApprovals: number;
    };

    expect(data.pendingApprovals).toBe(0);
  });
});
