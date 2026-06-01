// Dashboard loader — project list / overview (MVP §15).
//
// Loads the projects visible to the current user (GET /projects), then derives
// a per-project unresolved-issue count by listing each project's unresolved
// issues (GET /projects/{id}/issues?status=unresolved). Teams are loaded too so
// the create-project entry point can offer a team to attach the new project to.
//
// NOTE: the count is derived client-side because GET /projects does not yet
// return aggregate issue counts; see followups for the ideal backend field.

import { projects as projectsApi, teams as teamsApi } from '$lib/api';
import type { Project, TeamSummary } from '$lib/api';
import type { PageLoad } from './$types';

export interface ProjectOverview {
  project: Project;
  /** Number of unresolved issues, or `null` if the count could not be loaded. */
  unresolvedCount: number | null;
}

export const load: PageLoad = async ({ fetch }) => {
  const [projects, teams] = await Promise.all([
    projectsApi.list({ fetch }),
    teamsApi.list({ fetch }).catch((): TeamSummary[] => [])
  ]);

  const overviews: ProjectOverview[] = await Promise.all(
    projects.map(async (project) => {
      try {
        const issues = await projectsApi.issues(
          project.id,
          { status: 'unresolved', limit: 100 },
          { fetch }
        );
        return { project, unresolvedCount: issues.length };
      } catch {
        return { project, unresolvedCount: null };
      }
    })
  );

  return { overviews, teams };
};
