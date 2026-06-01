// Team detail (MVP §15 Teams): members and assigned projects. Viewable by any
// authenticated user; rename/delete and member management are instance-admin
// only (the backend enforces AdminUser on PATCH/DELETE/members routes).
//
// For admins we also load the full user list so the "add member" picker can
// resolve a user id. That endpoint (GET /admin/users) is admin-only, so we only
// call it when the current user is an admin.

import { admin, teams, type AdminUser } from '$lib/api';
import { requireUser } from '$lib/guards';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, parent, url, fetch }) => {
  const { user } = await parent();
  const authed = requireUser(user, url.pathname + url.search);

  const team = await teams.get(params.id, { fetch });

  let users: AdminUser[] = [];
  if (authed.is_admin) {
    users = await admin.users({ fetch });
  }

  return { team, users };
};
