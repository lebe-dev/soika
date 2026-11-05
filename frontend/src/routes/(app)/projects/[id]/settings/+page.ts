// Tag-mute rules are project-specific; team choices come from the bootstrap.
import { projects, notifications } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, parent, fetch }) => {
  const { user, config } = await parent();
  const [muteRules, alertmanagerInterval] = await Promise.all([
    projects.muteRules(params.id, { fetch }),
    notifications.projectInterval(params.id, { fetch })
  ]);
  const canMove = user?.instance_role === 'owner' || user?.instance_role === 'manager';
  return { alertmanagerInterval, muteRules, teamOptions: canMove ? (config?.teams ?? []) : [] };
};
