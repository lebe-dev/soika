// Tests for the team-detail page load: it always loads the team, but only
// fetches the instance-wide user list (manager-only endpoint) when the current
// user is an instance manager (Owner | Manager). Plain members get an empty
// `users` array and admin.users is never called. 404/403 on the team map to
// SvelteKit errors.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api/client';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    teams: { get: vi.fn() },
    admin: { users: vi.fn() }
  };
});

vi.mock('@sveltejs/kit', async () => {
  const actual = await vi.importActual<typeof import('@sveltejs/kit')>('@sveltejs/kit');
  return {
    ...actual,
    error: (status: number, body?: unknown) => {
      throw { status, body };
    },
    redirect: (status: number, location: string) => {
      throw { status, location };
    }
  };
});

import { admin, teams } from '$lib/api';
import { load } from './+page';

const teamGet = vi.mocked(teams.get);
const adminUsers = vi.mocked(admin.users);

const team = { id: 't1', name: 'Team' } as unknown as Awaited<ReturnType<typeof teams.get>>;

function callLoad(user: unknown) {
  return (load as (e: unknown) => Promise<unknown>)({
    params: { id: 't1' },
    parent: async () => ({ user }),
    url: new URL('https://app.test/teams/t1'),
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('(app)/teams/[id] +page load', () => {
  it('skips admin.users for a plain member and returns an empty users list', async () => {
    teamGet.mockResolvedValue(team);

    const data = (await callLoad({ id: 'u1', instance_role: 'member' })) as {
      team: unknown;
      users: unknown[];
    };

    expect(data.team).toBe(team);
    expect(data.users).toEqual([]);
    expect(adminUsers).not.toHaveBeenCalled();
  });

  it('calls admin.users for a manager and returns the resolved list', async () => {
    teamGet.mockResolvedValue(team);
    adminUsers.mockResolvedValue([{ id: 'u1' }, { id: 'u2' }] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'manager' })) as {
      team: unknown;
      users: unknown[];
    };

    expect(adminUsers).toHaveBeenCalledTimes(1);
    expect(data.users).toEqual([{ id: 'u1' }, { id: 'u2' }]);
  });

  it('calls admin.users for an owner too', async () => {
    teamGet.mockResolvedValue(team);
    adminUsers.mockResolvedValue([{ id: 'u1' }] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'owner' })) as { users: unknown[] };

    expect(adminUsers).toHaveBeenCalledTimes(1);
    expect(data.users).toEqual([{ id: 'u1' }]);
  });

  it('redirects to /login when there is no authenticated user', async () => {
    await expect(callLoad(null)).rejects.toMatchObject({ status: 307 });
    expect(teamGet).not.toHaveBeenCalled();
  });

  it('maps a 404 from teams.get to error(404)', async () => {
    teamGet.mockRejectedValue(new ApiError(404, 'Team not found', null));

    await expect(callLoad({ id: 'u1', instance_role: 'member' })).rejects.toMatchObject({
      status: 404
    });
  });

  it('maps a 403 from teams.get to error(403)', async () => {
    teamGet.mockRejectedValue(new ApiError(403, 'No access', null));

    await expect(callLoad({ id: 'u1', instance_role: 'member' })).rejects.toMatchObject({
      status: 403
    });
  });

  it('rethrows a non-ApiError failure untouched', async () => {
    const boom = new TypeError('network down');
    teamGet.mockRejectedValue(boom);

    await expect(callLoad({ id: 'u1', instance_role: 'member' })).rejects.toBe(boom);
  });
});
