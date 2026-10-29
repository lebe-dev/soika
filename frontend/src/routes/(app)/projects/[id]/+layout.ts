import { translate } from '$lib/i18n';
// Project area loader. Loads everything the project area needs in one shot when
// the project page is opened: the project (with its embedded default issue
// list) plus the caller's *effective* role for the project. Child tabs (Issues,
// SDK Setup, Settings) then render from this shared data without firing their
// own requests on tab switch. Access is enforced server-side; a 403/404
// surfaces as a SvelteKit error.
//
// Under the team-only access model there is no per-project membership: a
// project's access (and admin rights) come from membership in its owning team.
// We resolve the caller's effective role here — `admin` when they are an
// instance manager or a Team Admin of the owning team, otherwise `member` — so
// the settings page can gate its admin actions without a separate request. This
// load re-runs on `invalidateAll()` after a mutation, keeping every tab fresh.

import { error } from '@sveltejs/kit';
import { projects, ApiError, type Role } from '$lib/api';
import type { LayoutLoad } from './$types';

export const load: LayoutLoad = async ({ params, parent, fetch }) => {
  const { user, config } = await parent();

  try {
    // Project detail embeds the default (unresolved) issue list, so the issues
    // page can render its initial view without a separate request.
    const project = await projects.get(params.id, { fetch });

    // The bootstrap carries the caller's team role; no member directory read is needed.
    const effectiveRole: Role =
      user?.instance_role === 'owner' ||
      user?.instance_role === 'manager' ||
      config?.teams?.find((team) => team.id === project.team_id)?.role === 'admin'
        ? 'admin'
        : 'member';

    return { project, issues: project.issues, effectiveRole };
  } catch (err) {
    if (err instanceof ApiError) {
      if (err.isNotFound) error(404, translate('project_not_found'));
      if (err.isForbidden) error(403, translate('no_project_access'));
    }
    throw err;
  }
};
