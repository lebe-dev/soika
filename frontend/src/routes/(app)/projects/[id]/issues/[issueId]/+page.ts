// Issue detail loader. Fetches the issue (with its latest event)
// and the recent events list for event navigation in the stacktrace viewer.

import { error } from '@sveltejs/kit';
import { issues, ApiError } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, fetch }) => {
  try {
    const [issue, events] = await Promise.all([
      issues.get(params.issueId, { fetch }),
      issues.events(params.issueId, 50, { fetch })
    ]);
    return { issue, events };
  } catch (err) {
    if (err instanceof ApiError) {
      if (err.isNotFound) error(404, 'Issue not found');
      if (err.isForbidden) error(403, 'You do not have access to this issue');
    }
    throw err;
  }
};
