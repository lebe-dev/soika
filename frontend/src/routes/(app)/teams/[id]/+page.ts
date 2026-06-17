// Team detail (Teams): members and assigned projects. Viewable by instance
// managers (Owner | Manager) and by members of the team; rename/delete is
// instance-manager only, while managing membership/roles/invites is allowed for
// an instance manager OR a Team Admin of this team. The backend enforces all of
// this (AdminUser on rename/delete; require_team_manager on member/invite ops;
// GET /teams/{id} returns 403 to non-members).
//
// The "add member" picker needs the full user list to resolve a user id. That
// endpoint (GET /admin/users) is instance-manager only, so we only call it for
// a manager; a Team Admin who is only an instance Member can manage existing
// members/roles but cannot add arbitrary users (the picker is hidden).

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
    if (authed.instance_role === 'owner' || authed.instance_role === 'manager') {
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
