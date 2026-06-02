// Project settings loader. Loads members and pending invites for
// the project. The project itself comes from the parent layout load. Listing
// invites requires project admin; non-admin members get an empty invites list
// (the API returns 403, which we treat as "no invites visible").

import { projects, ApiError } from '$lib/api';
import type { Invite, ProjectMember } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, fetch }) => {
  const members = await projects.members(params.id, { fetch }).catch((err): ProjectMember[] => {
    if (err instanceof ApiError && (err.isForbidden || err.isUnauthorized)) return [];
    throw err;
  });

  const invites = await projects.invites(params.id, { fetch }).catch((err): Invite[] => {
    // Invite listing is admin-only; members simply don't see the section.
    if (err instanceof ApiError && (err.isForbidden || err.isUnauthorized)) return [];
    throw err;
  });

  return { members, invites };
};
