// Project area loader. Loads everything the project area needs in
// one shot when the project page is opened: the project (with its embedded
// default issue list) plus members and invites. Child tabs (Issues, SDK Setup,
// Settings) then render from this shared data without firing their own requests
// on tab switch. Membership/access is enforced server-side; a 403/404 surfaces
// as a SvelteKit error.
//
// Members/invites listing is admin-only; non-admin members get an empty list
// (the API returns 403, which we treat as "not visible"). This load re-runs on
// `invalidateAll()` after a mutation, keeping every tab fresh.

import { error } from '@sveltejs/kit';
import { projects, ApiError } from '$lib/api';
import type { Invite, ProjectMember } from '$lib/api';
import type { LayoutLoad } from './$types';

export const load: LayoutLoad = async ({ params, fetch }) => {
  try {
    // Project detail embeds the default (unresolved) issue list, so the issues
    // page can render its initial view without a separate request. Members and
    // invites load in parallel so the project area is fully populated up front.
    const [project, members, invites] = await Promise.all([
      projects.get(params.id, { fetch }),
      projects.members(params.id, { fetch }).catch((err): ProjectMember[] => {
        if (err instanceof ApiError && (err.isForbidden || err.isUnauthorized)) return [];
        throw err;
      }),
      projects.invites(params.id, { fetch }).catch((err): Invite[] => {
        // Invite listing is admin-only; members simply don't see the section.
        if (err instanceof ApiError && (err.isForbidden || err.isUnauthorized)) return [];
        throw err;
      })
    ]);
    return { project, issues: project.issues, members, invites };
  } catch (err) {
    if (err instanceof ApiError) {
      if (err.isNotFound) error(404, 'Project not found');
      if (err.isForbidden) error(403, 'You do not have access to this project');
    }
    throw err;
  }
};
