// Shared route guards for SvelteKit `load` functions (MVP §10, §15).
//
// SPA mode (ssr=false): these run in the browser. The root layout already
// loaded the current user; child loads read it from `parent()` and redirect
// when access is not permitted.

import { redirect } from '@sveltejs/kit';
import type { User } from '$lib/api';

/** Require an authenticated session; redirect to /login (preserving target). */
export function requireUser(user: User | null | undefined, currentPath: string): User {
  if (!user) {
    const next = encodeURIComponent(currentPath);
    redirect(307, `/login?next=${next}`);
  }
  return user;
}

/** Require the instance admin (§11); redirect to the dashboard otherwise. */
export function requireAdmin(user: User | null | undefined, currentPath: string): User {
  const authed = requireUser(user, currentPath);
  if (!authed.is_admin) {
    redirect(307, '/');
  }
  return authed;
}
