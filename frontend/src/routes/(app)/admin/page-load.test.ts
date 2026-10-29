import { beforeEach, describe, expect, it, vi } from 'vitest';

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

function callLoad(
  user: unknown,
  config: unknown = {
    admin: { settings: { name: 'soika' }, users: [{ id: 'u1' }] },
    teams: [{ id: 't1' }]
  }
) {
  return (load as (e: unknown) => Promise<unknown>)({
    parent: async () => ({ user, config }),
    url: new URL('https://app.test/admin'),
    fetch: vi.fn()
  });
}

beforeEach(() => vi.clearAllMocks());

describe('(app)/admin +page load', () => {
  it.each(['owner', 'manager'])(
    'uses the shared configuration for %s without requests',
    async (instance_role) => {
      const result = await callLoad({ id: 'u1', instance_role });
      expect(result).toEqual({
        settings: { name: 'soika' },
        users: [{ id: 'u1' }],
        teams: [{ id: 't1' }]
      });
      expect(settingsGet).not.toHaveBeenCalled();
      expect(adminUsers).not.toHaveBeenCalled();
      expect(teamsList).not.toHaveBeenCalled();
    }
  );
  it('redirects members', async () => {
    await expect(callLoad({ instance_role: 'member' })).rejects.toMatchObject({
      status: 307,
      location: '/'
    });
  });
  it('redirects anonymous visitors', async () => {
    await expect(callLoad(null)).rejects.toMatchObject({ status: 307 });
  });
  it('surfaces an unavailable admin slice', async () => {
    await expect(callLoad({ instance_role: 'owner' }, {})).rejects.toMatchObject({ status: 503 });
  });
});
