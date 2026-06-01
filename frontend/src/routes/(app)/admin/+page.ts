// Admin area guard + data load: requires the instance admin (§11, §14).
// Non-admins are redirected to the dashboard. Loads service settings, the
// instance-wide user list, and a teams overview (MVP §14, §15).

import { api, ApiError } from '$lib/api';
import { requireAdmin } from '$lib/guards';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ parent, url, fetch }) => {
  const { user } = await parent();
  requireAdmin(user, url.pathname + url.search);

  const [settings, users, teams] = await Promise.all([
    api.settings.get({ fetch }),
    api.admin.users({ fetch }),
    api.teams.list({ fetch }).catch((err) => {
      // Teams overview is supplementary; tolerate a missing/forbidden list
      // rather than failing the whole admin page.
      if (err instanceof ApiError) return [];
      throw err;
    })
  ]);

  return { settings, users, teams };
};
