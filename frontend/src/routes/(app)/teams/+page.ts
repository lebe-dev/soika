// Teams list (Teams). Any authenticated user may view the team list;
// create/edit/delete are gated to the instance admin in the UI (the backend
// enforces it as well — POST/PATCH/DELETE /teams require AdminUser).

import { teams } from '$lib/api';
import { requireUser } from '$lib/guards';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent, url, fetch }) => {
  const { user } = await parent();
  requireUser(user, url.pathname + url.search);
  const list = await teams.list({ fetch });
  return { teams: list };
};
