// Tests for the teams-list page load: any authenticated user may view the
// list (unauthenticated visitors are redirected to /login); the list comes
// from teams.list.

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    teams: { list: vi.fn() }
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

import { teams } from '$lib/api';
import { load } from './+page';

const teamsList = vi.mocked(teams.list);

function callLoad(user: unknown) {
  return (load as (e: unknown) => Promise<unknown>)({
    parent: async () => ({ user }),
    url: new URL('https://app.test/teams'),
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('(app)/teams +page load', () => {
  it('redirects to /login when there is no authenticated user', async () => {
    await expect(callLoad(null)).rejects.toMatchObject({ status: 307 });
    expect(teamsList).not.toHaveBeenCalled();
  });

  it('returns the team list for an authenticated user', async () => {
    teamsList.mockResolvedValue([{ id: 't1' }, { id: 't2' }] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'member' })) as { teams: unknown[] };

    expect(teamsList).toHaveBeenCalledTimes(1);
    expect(data.teams).toEqual([{ id: 't1' }, { id: 't2' }]);
  });
});
