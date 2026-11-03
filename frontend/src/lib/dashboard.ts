import { translate } from './i18n';

// Dashboard project-list helpers.
//
// Pure functions behind the project grid on the dashboard: which projects are
// visible for the current search/team/"with issues" filters and in which order,
// plus the option list for the team filter. Kept out of the component so the
// ranking rules are unit-testable.

/**
 * The shape the helpers need from a dashboard row — structurally satisfied by
 * the route's `ProjectOverview` (see `routes/(app)/+page.ts`), so `lib` does
 * not depend on a route module.
 */
export interface DashboardProject {
  project: { id: string; name: string; team_id: string; orphaned_at?: string | null };
  /** Number of unresolved issues for the project. */
  unresolvedCount: number;
  /** RFC 3339 timestamp of the most recent unresolved issue; null when none. */
  lastIssueAt: string | null;
}

/**
 * A project removed from the deployment configuration (`orphaned_at` set):
 * it no longer accepts events and is shown as disabled.
 */
export function isDisabledProject(project: { orphaned_at?: string | null }): boolean {
  return project.orphaned_at != null;
}

/**
 * Ordering of the project grid. Disabled projects always sink to the bottom
 * (name-sorted); among the rest, favorites float to the top in both modes.
 */
export type SortMode = 'activity' | 'name';

export interface ProjectViewOptions {
  /** Free-text project-name search; empty (or blank) matches everything. */
  query: string;
  /** Team id to restrict to; empty string means "all teams". */
  teamId: string;
  /** When set, hide projects that have no unresolved issues. */
  onlyWithIssues: boolean;
  sortMode: SortMode;
  favoritedIds: Set<string>;
}

/** Team option for the dashboard team filter. */
export interface TeamOption {
  id: string;
  name: string;
}

/** Apply the dashboard filters and ordering; the input array is left untouched. */
export function visibleProjects<T extends DashboardProject>(
  overviews: readonly T[],
  { query, teamId, onlyWithIssues, sortMode, favoritedIds }: ProjectViewOptions
): T[] {
  const q = query.toLowerCase().trim();

  const filtered = overviews.filter((o) => {
    if (q !== '' && !o.project.name.toLowerCase().includes(q)) return false;
    if (teamId !== '' && o.project.team_id !== teamId) return false;
    if (onlyWithIssues && o.unresolvedCount === 0) return false;
    return true;
  });

  return filtered.sort((a, b) => {
    const aOff = isDisabledProject(a.project) ? 1 : 0;
    const bOff = isDisabledProject(b.project) ? 1 : 0;
    if (aOff !== bOff) return aOff - bOff;
    if (aOff) return a.project.name.localeCompare(b.project.name);

    const aFav = favoritedIds.has(a.project.id) ? 1 : 0;
    const bFav = favoritedIds.has(b.project.id) ? 1 : 0;
    if (bFav !== aFav) return bFav - aFav;

    if (sortMode === 'activity') {
      const aHas = a.unresolvedCount > 0 ? 1 : 0;
      const bHas = b.unresolvedCount > 0 ? 1 : 0;
      if (bHas !== aHas) return bHas - aHas;
      if (a.lastIssueAt && b.lastIssueAt) {
        return b.lastIssueAt.localeCompare(a.lastIssueAt);
      }
    }

    return a.project.name.localeCompare(b.project.name);
  });
}

/**
 * Teams worth offering in the filter: those owning at least one of the given
 * projects, name-sorted. A project whose team is absent from `teams` (possible
 * when the bootstrap team list is narrower than the project list) still gets an
 * option so its projects stay reachable.
 */
export function teamFilterOptions(
  overviews: readonly DashboardProject[],
  teams: readonly TeamOption[]
): TeamOption[] {
  const byId = new Map(teams.map((t) => [t.id, t.name]));
  const options = new Map<string, TeamOption>();

  for (const { project } of overviews) {
    if (options.has(project.team_id)) continue;
    options.set(project.team_id, {
      id: project.team_id,
      name: byId.get(project.team_id) ?? translate('unknown_team')
    });
  }

  return [...options.values()].sort((a, b) => a.name.localeCompare(b.name));
}
