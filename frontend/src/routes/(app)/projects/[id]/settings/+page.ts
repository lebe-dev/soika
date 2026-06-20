// Settings tab loader. The project, members and invites come from the parent
// project-area layout load; this adds the project's tag-mute rules (member+
// visible). Merges with the layout data, so `data.project` etc. stay available.
//
// Moving a project between teams is an instance-management action: only an
// instance Owner/Manager may do it, and they may target any team. So we fetch
// the full team list only for those callers; everyone else gets `teamOptions:
// []` and the settings page hides the "Owning team" control.

import { projects, teams, type TeamSummary } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, parent, fetch }) => {
  const { user } = await parent();
  const muteRules = await projects.muteRules(params.id, { fetch });

  const canMove = user?.instance_role === 'owner' || user?.instance_role === 'manager';
  let teamOptions: TeamSummary[] = [];
  if (canMove) {
    try {
      teamOptions = await teams.list({ fetch });
    } catch {
      // A failed team list must not break the settings page; the control is
      // simply hidden when there are no options to move to.
      teamOptions = [];
    }
  }

  return { muteRules, teamOptions };
};
