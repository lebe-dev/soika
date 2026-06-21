// Tests for the dashboard page load: it derives the project overviews and team
// list purely from the `/auth/config` bootstrap exposed by the root layout —
// no extra fetches. Each project row is split into `{ project, unresolvedCount,
// favorited, lastIssueAt }`; a missing config yields empty arrays.

import { describe, expect, it } from 'vitest';
import { load } from './+page';

function callLoad(config: unknown) {
  return (load as (e: unknown) => Promise<unknown>)({
    parent: async () => ({ config })
  });
}

describe('(app) +page load (dashboard)', () => {
  it('returns empty arrays when there is no config', async () => {
    const data = (await callLoad(null)) as { overviews: unknown[]; teams: unknown[] };
    expect(data.overviews).toEqual([]);
    expect(data.teams).toEqual([]);
  });

  it('returns empty arrays when config has no projects/teams', async () => {
    const data = (await callLoad({})) as { overviews: unknown[]; teams: unknown[] };
    expect(data.overviews).toEqual([]);
    expect(data.teams).toEqual([]);
  });

  it('maps each config project into an overview, separating the counts from the project', async () => {
    const data = (await callLoad({
      projects: [
        {
          id: 'p1',
          name: 'Proj One',
          unresolved_count: 3,
          favorited: true,
          last_issue_at: '2026-06-01T00:00:00Z'
        },
        {
          id: 'p2',
          name: 'Proj Two',
          unresolved_count: 0,
          favorited: false,
          last_issue_at: null
        }
      ],
      teams: [{ id: 't1', name: 'Team' }]
    })) as {
      overviews: Array<{
        project: { id: string; name: string; unresolved_count?: number };
        unresolvedCount: number;
        favorited: boolean;
        lastIssueAt: string | null;
      }>;
      teams: Array<{ id: string }>;
    };

    expect(data.overviews).toHaveLength(2);

    const [first, second] = data.overviews;
    expect(first.project).toEqual({ id: 'p1', name: 'Proj One' });
    expect(first.project.unresolved_count).toBeUndefined();
    expect(first.unresolvedCount).toBe(3);
    expect(first.favorited).toBe(true);
    expect(first.lastIssueAt).toBe('2026-06-01T00:00:00Z');

    expect(second.unresolvedCount).toBe(0);
    expect(second.favorited).toBe(false);
    expect(second.lastIssueAt).toBeNull();

    expect(data.teams).toEqual([{ id: 't1', name: 'Team' }]);
  });

  it('coerces a missing last_issue_at to null', async () => {
    const data = (await callLoad({
      projects: [{ id: 'p1', name: 'P', unresolved_count: 1, favorited: false }]
    })) as { overviews: Array<{ lastIssueAt: string | null }> };

    expect(data.overviews[0].lastIssueAt).toBeNull();
  });
});
