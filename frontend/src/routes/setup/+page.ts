// The root layout load runs untracked and does not rerun on client-side
// navigation, so a stray in-app link to /setup is bounced here instead: once
// the instance is initialized, setup is closed and visitors go to login.

import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent }) => {
  const { config } = await parent();
  if (config?.initialized) redirect(307, '/login');
};
