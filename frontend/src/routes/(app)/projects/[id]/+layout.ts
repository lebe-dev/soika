// Project area loader (MVP §8, §15). Loads the project once for the whole
// project route group (issues list, issue detail, setup, settings) so the
// header and child pages share it. Membership/access is enforced server-side;
// a 403/404 surfaces as a SvelteKit error.

import { error } from '@sveltejs/kit';
import { projects, ApiError } from '$lib/api';
import type { LayoutLoad } from './$types';

export const load: LayoutLoad = async ({ params, fetch }) => {
  try {
    const project = await projects.get(params.id, { fetch });
    return { project };
  } catch (err) {
    if (err instanceof ApiError) {
      if (err.isNotFound) error(404, 'Project not found');
      if (err.isForbidden) error(403, 'You do not have access to this project');
    }
    throw err;
  }
};
