// Tests for the project-settings page load: it always loads the project's
// tag-mute rules, and additionally fetches the full team list (for the "move
// project" control) only for instance Owner/Manager. A failed team list must
// not break the page — it falls back to an empty `teamOptions`.

import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    projects: { muteRules: vi.fn() },
    teams: { list: vi.fn() }
  };
});

import { projects, teams } from '$lib/api';
import { load } from './+page';

const muteRules = vi.mocked(projects.muteRules);
const teamsList = vi.mocked(teams.list);

function callLoad(user: unknown) {
  return (load as (e: unknown) => Promise<unknown>)({
    params: { id: 'p1' },
    parent: async () => ({ user }),
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  muteRules.mockResolvedValue([{ id: 'r1' }] as never);
});

describe('(app)/projects/[id]/settings +page load', () => {
  it('loads mute rules and skips the team list for a plain member', async () => {
    const data = (await callLoad({ id: 'u1', instance_role: 'member' })) as {
      muteRules: unknown[];
      teamOptions: unknown[];
    };

    expect(data.muteRules).toEqual([{ id: 'r1' }]);
    expect(data.teamOptions).toEqual([]);
    expect(teamsList).not.toHaveBeenCalled();
  });

  it('loads the team list for a manager', async () => {
    teamsList.mockResolvedValue([{ id: 't1' }, { id: 't2' }] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'manager' })) as {
      teamOptions: unknown[];
    };

    expect(teamsList).toHaveBeenCalledTimes(1);
    expect(data.teamOptions).toEqual([{ id: 't1' }, { id: 't2' }]);
  });

  it('loads the team list for an owner too', async () => {
    teamsList.mockResolvedValue([{ id: 't1' }] as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'owner' })) as {
      teamOptions: unknown[];
    };

    expect(teamsList).toHaveBeenCalledTimes(1);
    expect(data.teamOptions).toEqual([{ id: 't1' }]);
  });

  it('falls back to empty teamOptions when the team list fetch fails', async () => {
    teamsList.mockRejectedValue(new Error('boom'));

    const data = (await callLoad({ id: 'u1', instance_role: 'owner' })) as {
      teamOptions: unknown[];
    };

    expect(data.teamOptions).toEqual([]);
  });
});
