// Tests for the project-area layout load: it loads the project and resolves the
// caller's *effective* role (instance managers administer every project; others
// derive it from their role in the owning team). It maps 404/403 on the project
// to SvelteKit errors and rethrows non-ApiError failures untouched. A failed
// owning-team fetch falls back to the `member` role (the API still enforces the
// real check on writes).
//
// `@sveltejs/kit`'s `error()` throws (it never returns), so we assert on the
// thrown value's `status`. `$lib/api` is mocked, but we keep the REAL `ApiError`
// class so the load's `instanceof ApiError` branches match.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api/client';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    projects: { get: vi.fn() },
    teams: { get: vi.fn() }
  };
});

// `error(status, body)` throws a SvelteKit HttpError; mirror that so the load's
// `error(404, ...)` calls surface as throws with a readable `status`.
vi.mock('@sveltejs/kit', async () => {
  const actual = await vi.importActual<typeof import('@sveltejs/kit')>('@sveltejs/kit');
  return {
    ...actual,
    error: (status: number, body?: unknown) => {
      throw { status, body };
    }
  };
});

import { projects, teams } from '$lib/api';
import { load } from './+layout';

const get = vi.mocked(projects.get);
const teamGet = vi.mocked(teams.get);

const project = {
  id: 'p1',
  name: 'Proj',
  team_id: 't1',
  issues: [{ id: 'i1' }]
} as unknown as Awaited<ReturnType<typeof projects.get>>;

function callLoad(user: unknown) {
  return (load as (e: unknown) => Promise<unknown>)({
    params: { id: 'p1' },
    parent: async () => ({ user }),
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('(app)/projects/[id] +layout load', () => {
  it('returns the project, its embedded issues, and admin role for a manager (no team fetch)', async () => {
    get.mockResolvedValue(project);

    const data = (await callLoad({ id: 'u1', instance_role: 'manager' })) as {
      project: unknown;
      issues: unknown[];
      effectiveRole: string;
    };

    expect(data.project).toBe(project);
    expect(data.issues).toBe(project.issues);
    expect(data.effectiveRole).toBe('admin');
    expect(teamGet).not.toHaveBeenCalled();
  });

  it('resolves admin for a Team Admin of the owning team', async () => {
    get.mockResolvedValue(project);
    teamGet.mockResolvedValue({
      id: 't1',
      members: [{ id: 'u1', role: 'admin' }]
    } as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'member' })) as {
      effectiveRole: string;
    };

    expect(teamGet).toHaveBeenCalledWith('t1', expect.anything());
    expect(data.effectiveRole).toBe('admin');
  });

  it('resolves member for a Contributor of the owning team', async () => {
    get.mockResolvedValue(project);
    teamGet.mockResolvedValue({
      id: 't1',
      members: [{ id: 'u1', role: 'contributor' }]
    } as never);

    const data = (await callLoad({ id: 'u1', instance_role: 'member' })) as {
      effectiveRole: string;
    };

    expect(data.effectiveRole).toBe('member');
  });

  it('falls back to member when the owning-team fetch fails', async () => {
    get.mockResolvedValue(project);
    teamGet.mockRejectedValue(new ApiError(403, 'forbidden', null));

    const data = (await callLoad({ id: 'u1', instance_role: 'member' })) as {
      effectiveRole: string;
    };

    expect(data.effectiveRole).toBe('member');
  });

  it('throws error(404) when projects.get reports a 404', async () => {
    get.mockRejectedValue(new ApiError(404, 'Project not found', null));

    await expect(callLoad({ id: 'u1', instance_role: 'member' })).rejects.toMatchObject({
      status: 404
    });
  });

  it('throws error(403) when projects.get reports a 403', async () => {
    get.mockRejectedValue(new ApiError(403, 'No access', null));

    await expect(callLoad({ id: 'u1', instance_role: 'member' })).rejects.toMatchObject({
      status: 403
    });
  });

  it('rethrows a non-ApiError failure from projects.get untouched', async () => {
    const boom = new TypeError('network down');
    get.mockRejectedValue(boom);

    await expect(callLoad({ id: 'u1', instance_role: 'member' })).rejects.toBe(boom);
  });
});
