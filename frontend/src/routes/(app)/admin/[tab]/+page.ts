import { load as loadAdmin } from '../+page';
import type { PageLoad } from './$types';

export const load = ((event) => loadAdmin(event)) satisfies PageLoad;
