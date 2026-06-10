// Settings tab loader. The project, members and invites come from the parent
// project-area layout load; this adds the project's tag-mute rules (member+
// visible). Merges with the layout data, so `data.project` etc. stay available.

import { projects } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, fetch }) => {
  const muteRules = await projects.muteRules(params.id, { fetch });
  return { muteRules };
};
