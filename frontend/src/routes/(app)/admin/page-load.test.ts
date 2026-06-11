// Tests for the admin-area page load: it requires the instance admin (redirects
// otherwise), loads settings + users + a teams overview in parallel, and treats
// the teams overview as supplementary — a 403/401 from teams.list yields [], but
// any other failure (e.g. a TypeError, or a 500) surfaces instead of being
// masked behind an empty list.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api/client';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    api: {
      ...actual.api,
      settings: { get: vi.fn() },
      admin: { users: vi.fn() },
      teams: { list: vi.fn() }
    }
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

import { api } from '$lib/api';
import { load } from './+page';

const settingsGet = vi.mocked(api.settings.get);
const adminUsers = vi.mocked(api.admin.users);
const teamsList = vi.mocked(api.teams.list);

function callLoad(user: unknown) {
  return (load as (e: unknown) => Promise<unknown>)({
    parent: async () => ({ user }),
    url: new URL('https://app.test/admin'),
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  settingsGet.mockResolvedValue({ name: 'soika' } as never);
  adminUsers.mockResolvedValue([{ id: 'u1' }] as never);
});

describe('(app)/admin +page load', () => {
  it('loads settings, users and teams for an admin', async () => {
    teamsList.mockResolvedValue([{ id: 't1' }] as never);

    const data = (await callLoad({ id: 'u1', is_admin: true })) as {
      settings: unknown;
      users: unknown[];
      teams: unknown[];
    };

    expect(data.settings).toEqual({ name: 'soika' });
    expect(data.users).toEqual([{ id: 'u1' }]);
    expect(data.teams).toEqual([{ id: 't1' }]);
  });

  it('redirects a non-admin user to the dashboard', async () => {
    await expect(callLoad({ id: 'u1', is_admin: false })).rejects.toMatchObject({
      status: 307,
      location: '/'
    });
  });

  it('redirects an unauthenticated user to /login', async () => {
    await expect(callLoad(null)).rejects.toMatchObject({ status: 307 });
  });

  it('returns [] teams on a forbidden teams.list', async () => {
    teamsList.mockRejectedValue(new ApiError(403, 'forbidden', null));

    const data = (await callLoad({ id: 'u1', is_admin: true })) as { teams: unknown[] };

    expect(data.teams).toEqual([]);
  });

  it('returns [] teams on an unauthorized teams.list', async () => {
    teamsList.mockRejectedValue(new ApiError(401, 'unauthorized', null));

    const data = (await callLoad({ id: 'u1', is_admin: true })) as { teams: unknown[] };

    expect(data.teams).toEqual([]);
  });

  it('rethrows a thrown TypeError from teams.list rather than masking it', async () => {
    const boom = new TypeError('network down');
    teamsList.mockRejectedValue(boom);

    await expect(callLoad({ id: 'u1', is_admin: true })).rejects.toBe(boom);
  });

  it('rethrows a non-403/401 ApiError (e.g. 500) from teams.list', async () => {
    teamsList.mockRejectedValue(new ApiError(500, 'boom', null));

    await expect(callLoad({ id: 'u1', is_admin: true })).rejects.toMatchObject({ status: 500 });
  });
});
