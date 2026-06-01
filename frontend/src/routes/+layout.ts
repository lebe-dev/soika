// Root layout load: this is a session-cookie SPA hitting the Rust JSON API,
// so we disable SSR (the server has no cookie context at prerender time) and
// run as a single-page app with a static fallback document (MVP §2.2).
//
// We fetch the current user once here so the layout, nav, and per-route guards
// can rely on `data.user` without each route re-fetching.

import { auth, type User } from '$lib/api';
import type { LayoutLoad } from './$types';

export const ssr = false;
export const prerender = false;

export const load: LayoutLoad = async ({ fetch }) => {
  let user: User | null = null;
  try {
    user = await auth.me({ fetch });
  } catch {
    // Network/Server error: treat as unauthenticated; route guards handle the
    // redirect. Avoids crashing the whole shell when the backend is unreachable.
    user = null;
  }
  return { user };
};
