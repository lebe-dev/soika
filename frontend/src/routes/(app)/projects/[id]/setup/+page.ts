// SDK setup loader. Fetches the DSN and per-language init snippets
// (Go, Rust, Svelte/JS, generic) for the project.

import { error } from '@sveltejs/kit';
import { projects, ApiError } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, fetch }) => {
  try {
    const setup = await projects.sdkSetup(params.id, { fetch });
    return { setup };
  } catch (err) {
    if (err instanceof ApiError && err.isForbidden) {
      error(403, 'You do not have access to this project');
    }
    throw err;
  }
};
