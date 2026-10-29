import { requireUser } from '$lib/guards';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent, url }) => {
  const { user, config } = await parent();
  requireUser(user, url.pathname + url.search);
  return { teams: config?.teams ?? [] };
};
