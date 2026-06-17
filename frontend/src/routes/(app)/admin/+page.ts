// Admin area guard + data load: requires an instance manager (Owner | Manager).
// Other users are redirected to the dashboard. Loads service settings, the
// instance-wide user list, and a teams overview.

import { api, ApiError } from '$lib/api';
import { requireInstanceManager } from '$lib/guards';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent, url, fetch }) => {
  const { user } = await parent();
  requireInstanceManager(user, url.pathname + url.search);

  const [settings, users, teams] = await Promise.all([
    api.settings.get({ fetch }),
    api.admin.users({ fetch }),
    api.teams.list({ fetch }).catch((err) => {
      // Teams overview is supplementary; tolerate a forbidden/unauthorized list
      // rather than failing the whole admin page. Real failures (404/500) still
      // surface — don't mask them behind an empty list.
      if (err instanceof ApiError && (err.isForbidden || err.isUnauthorized)) return [];
      throw err;
    })
  ]);

  return { settings, users, teams };
};
