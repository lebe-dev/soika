import { error } from '@sveltejs/kit';
import { requireInstanceManager } from '$lib/guards';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent, url }) => {
  const { user, config } = await parent();
  requireInstanceManager(user, url.pathname + url.search);
  if (!config?.admin) error(503, 'Configuration unavailable');
  return { settings: config.admin.settings, users: config.admin.users, teams: config.teams ?? [] };
};
