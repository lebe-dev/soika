// Guard for the authenticated app shell: every route under (app) requires a
// session. Unauthenticated visitors are redirected to /login.

import { requireUser } from '$lib/guards';
import type { LayoutLoad } from './$types';

export const load: LayoutLoad = async ({ parent, url }) => {
  const { user } = await parent();
  const authed = requireUser(user, url.pathname + url.search);
  return { user: authed };
};
