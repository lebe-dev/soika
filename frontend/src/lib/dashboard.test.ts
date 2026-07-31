// Tests for the dashboard project list helpers: the visible-project pipeline
// (search + team filter + "with issues" toggle + favorites-first sorting) and
// the team-filter option derivation.

import { describe, expect, it } from 'vitest';
import { teamFilterOptions, visibleProjects, type DashboardProject } from './dashboard';

function proj(
  id: string,
  name: string,
  teamId: string,
  unresolvedCount = 0,
  lastIssueAt: string | null = null
): DashboardProject {
  return { project: { id, name, team_id: teamId }, unresolvedCount, lastIssueAt };
}

const alpha = proj('p1', 'Alpha', 't1', 0);
const beta = proj('p2', 'Beta', 't1', 3, '2026-06-02T00:00:00Z');
const gamma = proj('p3', 'Gamma', 't2', 1, '2026-06-05T00:00:00Z');

const all = [alpha, beta, gamma];

function ids(items: DashboardProject[]): string[] {
  return items.map((o) => o.project.id);
}

describe('visibleProjects', () => {
  it('keeps everything (name-sorted) with no filters and no favorites in name mode', () => {
    const out = visibleProjects(all, {
      query: '',
      teamId: '',
      onlyWithIssues: false,
      sortMode: 'name',
      favoritedIds: new Set()
    });
    expect(ids(out)).toEqual(['p1', 'p2', 'p3']);
  });

  it('does not mutate the input array', () => {
    const input = [gamma, alpha, beta];
    visibleProjects(input, {
      query: '',
      teamId: '',
      onlyWithIssues: false,
      sortMode: 'name',
      favoritedIds: new Set()
    });
    expect(ids(input)).toEqual(['p3', 'p1', 'p2']);
  });

  it('filters by name, case-insensitively and ignoring surrounding whitespace', () => {
    const out = visibleProjects(all, {
      query: '  ALP ',
      teamId: '',
      onlyWithIssues: false,
      sortMode: 'name',
      favoritedIds: new Set()
    });
    expect(ids(out)).toEqual(['p1']);
  });

  it('filters by team when a team id is selected', () => {
    const out = visibleProjects(all, {
      query: '',
      teamId: 't1',
      onlyWithIssues: false,
      sortMode: 'name',
      favoritedIds: new Set()
    });
    expect(ids(out)).toEqual(['p1', 'p2']);
  });

  it('treats an empty team id as "all teams"', () => {
    const out = visibleProjects(all, {
      query: '',
      teamId: '',
      onlyWithIssues: false,
      sortMode: 'name',
      favoritedIds: new Set()
    });
    expect(out).toHaveLength(3);
  });

  it('drops projects without unresolved issues when onlyWithIssues is set', () => {
    const out = visibleProjects(all, {
      query: '',
      teamId: '',
      onlyWithIssues: true,
      sortMode: 'name',
      favoritedIds: new Set()
    });
    expect(ids(out)).toEqual(['p2', 'p3']);
  });

  it('combines team and with-issues filters', () => {
    const out = visibleProjects(all, {
      query: '',
      teamId: 't1',
      onlyWithIssues: true,
      sortMode: 'name',
      favoritedIds: new Set()
    });
    expect(ids(out)).toEqual(['p2']);
  });

  it('puts favorites first, whatever the sort mode', () => {
    const out = visibleProjects(all, {
      query: '',
      teamId: '',
      onlyWithIssues: false,
      sortMode: 'name',
      favoritedIds: new Set(['p3'])
    });
    expect(ids(out)).toEqual(['p3', 'p1', 'p2']);
  });

  it('in activity mode ranks projects with issues first, then by most recent issue', () => {
    const out = visibleProjects(all, {
      query: '',
      teamId: '',
      onlyWithIssues: false,
      sortMode: 'activity',
      favoritedIds: new Set()
    });
    expect(ids(out)).toEqual(['p3', 'p2', 'p1']);
  });

  it('falls back to name ordering in activity mode when a timestamp is missing', () => {
    const noTimestamp = proj('p4', 'Aardvark', 't2', 2, null);
    const out = visibleProjects([beta, noTimestamp], {
      query: '',
      teamId: '',
      onlyWithIssues: false,
      sortMode: 'activity',
      favoritedIds: new Set()
    });
    expect(ids(out)).toEqual(['p4', 'p2']);
  });
});

describe('teamFilterOptions', () => {
  it('returns only teams that own at least one of the given projects', () => {
    const out = teamFilterOptions(all, [
      { id: 't1', name: 'Core' },
      { id: 't2', name: 'Ops' },
      { id: 't3', name: 'Empty' }
    ]);
    expect(out).toEqual([
      { id: 't1', name: 'Core' },
      { id: 't2', name: 'Ops' }
    ]);
  });

  it('sorts the options by name', () => {
    const out = teamFilterOptions(all, [
      { id: 't2', name: 'Ops' },
      { id: 't1', name: 'Core' }
    ]);
    expect(out.map((t) => t.id)).toEqual(['t1', 't2']);
  });

  it('synthesizes an option for a team missing from the team list', () => {
    const out = teamFilterOptions([gamma], []);
    expect(out).toEqual([{ id: 't2', name: 'Unknown team' }]);
  });

  it('returns an empty list when there are no projects', () => {
    expect(teamFilterOptions([], [{ id: 't1', name: 'Core' }])).toEqual([]);
  });
});
