// Tests for the issue-detail page load: it fetches the issue (with its latest
// event) and the recent events list in parallel. A 404 maps to error(404), a
// 403 to error(403); any other failure rethrows untouched.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api/client';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    issues: { get: vi.fn(), events: vi.fn() }
  };
});

vi.mock('@sveltejs/kit', async () => {
  const actual = await vi.importActual<typeof import('@sveltejs/kit')>('@sveltejs/kit');
  return {
    ...actual,
    error: (status: number, body?: unknown) => {
      throw { status, body };
    }
  };
});

import { issues } from '$lib/api';
import { load } from './+page';

const issuesGet = vi.mocked(issues.get);
const issuesEvents = vi.mocked(issues.events);

function callLoad() {
  return (load as (e: unknown) => Promise<unknown>)({
    params: { id: 'p1', issueId: 'i1' },
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('(app)/projects/[id]/issues/[issueId] +page load', () => {
  it('returns the issue and its events on success', async () => {
    const issue = { id: 'i1' } as never;
    const events = [{ id: 'e1' }, { id: 'e2' }] as never;
    issuesGet.mockResolvedValue(issue);
    issuesEvents.mockResolvedValue(events);

    const data = (await callLoad()) as { issue: unknown; events: unknown[] };

    expect(data.issue).toBe(issue);
    expect(data.events).toBe(events);
    expect(issuesEvents).toHaveBeenCalledWith('i1', 50, expect.anything());
  });

  it('maps a 404 to error(404)', async () => {
    issuesGet.mockRejectedValue(new ApiError(404, 'Issue not found', null));
    issuesEvents.mockResolvedValue([] as never);

    await expect(callLoad()).rejects.toMatchObject({ status: 404 });
  });

  it('maps a 403 to error(403)', async () => {
    issuesGet.mockRejectedValue(new ApiError(403, 'No access', null));
    issuesEvents.mockResolvedValue([] as never);

    await expect(callLoad()).rejects.toMatchObject({ status: 403 });
  });

  it('rethrows a non-ApiError failure untouched', async () => {
    const boom = new TypeError('network down');
    issuesGet.mockRejectedValue(boom);
    issuesEvents.mockResolvedValue([] as never);

    await expect(callLoad()).rejects.toBe(boom);
  });
});
