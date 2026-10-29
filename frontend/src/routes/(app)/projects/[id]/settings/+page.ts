// Tag-mute rules are project-specific; team choices come from the bootstrap.
import { projects } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, parent, fetch }) => {
  const { user, config } = await parent();
  const muteRules = await projects.muteRules(params.id, { fetch });
  const canMove = user?.instance_role === 'owner' || user?.instance_role === 'manager';
  return { muteRules, teamOptions: canMove ? (config?.teams ?? []) : [] };
};
