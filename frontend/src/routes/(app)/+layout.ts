import { requireUser } from '$lib/guards';
import type { LayoutLoad } from './$types';

export const load: LayoutLoad = async ({ parent, url, untrack }) => {
  const { user, config } = await parent();
  const authed = requireUser(
    user,
    untrack(() => url.pathname + url.search)
  );
  const pendingApprovals = config?.admin?.users.filter((u) => u.status === 'pending').length ?? 0;
  return { user: authed, pendingApprovals };
};
