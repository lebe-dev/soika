// Team detail (Teams): members and assigned projects. Viewable by instance
// admins and by members of the team; rename/delete and member management are
// instance-admin only (the backend enforces AdminUser on PATCH/DELETE/members
// routes, and GET /teams/{id} returns 403 to non-members).
//
// For admins we also load the full user list so the "add member" picker can
// resolve a user id. That endpoint (GET /admin/users) is admin-only, so we only
// call it when the current user is an admin.

import { error } from '@sveltejs/kit';
import { admin, teams, ApiError, type AdminUser } from '$lib/api';
import { requireUser } from '$lib/guards';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, parent, url, fetch }) => {
  const { user } = await parent();
  const authed = requireUser(user, url.pathname + url.search);

  try {
    const team = await teams.get(params.id, { fetch });

    let users: AdminUser[] = [];
    if (authed.is_admin) {
      users = await admin.users({ fetch });
    }

    return { team, users };
  } catch (err) {
    if (err instanceof ApiError) {
      if (err.isNotFound) error(404, 'Team not found');
      if (err.isForbidden) error(403, 'You do not have access to this team');
    }
    throw err;
  }
};
