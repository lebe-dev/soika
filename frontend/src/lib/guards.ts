// Shared route guards for SvelteKit `load` functions.
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

/**
 * Require a user who can manage the instance (`Owner | Manager`); redirect to
 * the dashboard otherwise. Gates the admin area, consistent with the backend
 * `AdminUser` extractor (which accepts `Owner | Manager`).
 */
export function requireInstanceManager(user: User | null | undefined, currentPath: string): User {
  const authed = requireUser(user, currentPath);
  if (authed.instance_role !== 'owner' && authed.instance_role !== 'manager') {
    redirect(307, '/');
  }
  return authed;
}
