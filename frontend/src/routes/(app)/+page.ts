// Dashboard loader — project list / overview.
//
// All the data the dashboard needs (projects with their unresolved-issue
// counts, plus team summaries for the create-project picker) is embedded in the
// `/auth/config` bootstrap fetched once by the root layout. We read it from the
// parent layout data instead of fanning out to `/projects`, `/teams` and one
// `/issues` call per project.

import type { Project, TeamSummary } from '$lib/api';
import type { PageLoad } from './$types';

export interface ProjectOverview {
  project: Project;
  /** Number of unresolved issues for the project. */
  unresolvedCount: number;
}

export const load: PageLoad = async ({ parent }) => {
  const { config } = await parent();

  const overviews: ProjectOverview[] = (config?.projects ?? []).map(
    ({ unresolved_count, ...project }) => ({ project, unresolvedCount: unresolved_count })
  );
  const teams: TeamSummary[] = config?.teams ?? [];

  return { overviews, teams };
};
