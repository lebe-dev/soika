// Guard for the authenticated app shell: every route under (app) requires a
// session. Unauthenticated visitors are redirected to /login.

import { api } from '$lib/api';
import { requireUser } from '$lib/guards';
import type { LayoutLoad } from './$types';

// The URL is only needed for the login redirect target, so it is read
// untracked: otherwise every navigation would rerun this load and refetch the
// user list.
export const load: LayoutLoad = async ({ parent, url, untrack, fetch }) => {
  const { user } = await parent();
  const authed = requireUser(
    user,
    untrack(() => url.pathname + url.search)
  );

  // Surface the count of accounts awaiting approval on the Admin nav item.
  // Only instance managers (Owner | Manager) can see (and act on) it, so skip
  // the call entirely for everyone else; a failure here must not break the app
  // shell.
  let pendingApprovals = 0;
  if (authed.instance_role === 'owner' || authed.instance_role === 'manager') {
    try {
      const users = await api.admin.users({ fetch });
      pendingApprovals = users.filter((u) => u.status === 'pending').length;
    } catch {
      pendingApprovals = 0;
    }
  }

  return { user: authed, pendingApprovals };
};
