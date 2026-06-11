// Tests for the project-area layout load: how it tolerates a forbidden
// members/invites listing (non-admin members) while still loading the project,
// how it maps 404/403 on the project itself to SvelteKit errors, and how it
// rethrows non-ApiError failures untouched.
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
    projects: {
      get: vi.fn(),
      members: vi.fn(),
      invites: vi.fn()
    }
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

import { projects } from '$lib/api';
import { load } from './+layout';

const get = vi.mocked(projects.get);
const members = vi.mocked(projects.members);
const invites = vi.mocked(projects.invites);

const project = {
  id: 'p1',
  name: 'Proj',
  issues: [{ id: 'i1' }]
} as unknown as Awaited<ReturnType<typeof projects.get>>;

function callLoad() {
  return (load as (e: unknown) => Promise<unknown>)({
    params: { id: 'p1' },
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('(app)/projects/[id] +layout load', () => {
  it('returns the project plus its embedded issues, members and invites', async () => {
    get.mockResolvedValue(project);
    members.mockResolvedValue([{ user_id: 'u1' }] as never);
    invites.mockResolvedValue([{ token: 't1' }] as never);

    const data = await callLoad();

    expect(data).toEqual({
      project,
      issues: project.issues,
      members: [{ user_id: 'u1' }],
      invites: [{ token: 't1' }]
    });
  });

  it('throws error(404) when projects.get reports a 404', async () => {
    get.mockRejectedValue(new ApiError(404, 'Project not found', null));
    members.mockResolvedValue([] as never);
    invites.mockResolvedValue([] as never);

    await expect(callLoad()).rejects.toMatchObject({ status: 404 });
  });

  it('throws error(403) when projects.get reports a 403', async () => {
    get.mockRejectedValue(new ApiError(403, 'No access', null));
    members.mockResolvedValue([] as never);
    invites.mockResolvedValue([] as never);

    await expect(callLoad()).rejects.toMatchObject({ status: 403 });
  });

  it('yields empty members/invites on a 403 while the project still loads', async () => {
    get.mockResolvedValue(project);
    members.mockRejectedValue(new ApiError(403, 'forbidden', null));
    invites.mockRejectedValue(new ApiError(403, 'forbidden', null));

    const data = (await callLoad()) as { project: unknown; members: unknown[]; invites: unknown[] };

    expect(data.project).toBe(project);
    expect(data.members).toEqual([]);
    expect(data.invites).toEqual([]);
  });

  it('also tolerates a 401 on members/invites with empty lists', async () => {
    get.mockResolvedValue(project);
    members.mockRejectedValue(new ApiError(401, 'unauthorized', null));
    invites.mockRejectedValue(new ApiError(401, 'unauthorized', null));

    const data = (await callLoad()) as { members: unknown[]; invites: unknown[] };

    expect(data.members).toEqual([]);
    expect(data.invites).toEqual([]);
  });

  it('rethrows a non-ApiError failure from projects.get untouched', async () => {
    const boom = new TypeError('network down');
    get.mockRejectedValue(boom);
    members.mockResolvedValue([] as never);
    invites.mockResolvedValue([] as never);

    await expect(callLoad()).rejects.toBe(boom);
  });

  it('rethrows a non-403/401 ApiError from members (e.g. 500) instead of swallowing it', async () => {
    get.mockResolvedValue(project);
    members.mockRejectedValue(new ApiError(500, 'boom', null));
    invites.mockResolvedValue([] as never);

    await expect(callLoad()).rejects.toMatchObject({ status: 500 });
  });
});
