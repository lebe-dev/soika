// Root layout load: this is a session-cookie SPA hitting the Rust JSON API,
// so we disable SSR (the server has no cookie context at prerender time) and
// run as a single-page app with a static fallback document.
//
// We fetch the current user and the public auth config once here so the layout,
// nav, and per-route guards can rely on `data.user` / `data.config` without
// each route re-fetching. The config also drives first-run routing:
// an uninitialized instance is sent to `/setup`; once initialized, `/setup`
// bounces back to the login form.

import { redirect } from '@sveltejs/kit';
import { auth, type AuthConfig, type User } from '$lib/api';
import type { LayoutLoad } from './$types';

export const ssr = false;
export const prerender = false;

export const load: LayoutLoad = async ({ fetch, url }) => {
  let user: User | null = null;
  let config: AuthConfig | null = null;

  // Fetch both up front; tolerate failures so a backend hiccup doesn't trap the
  // user. Redirects are decided AFTER this block — `redirect()` throws, and we
  // must not swallow it inside the catch.
  try {
    [user, config] = await Promise.all([auth.me({ fetch }), auth.config({ fetch })]);
  } catch {
    // Network/server error: treat as unauthenticated with unknown config; route
    // guards handle auth redirects and we skip first-run routing below.
  }

  if (config) {
    const onSetup = url.pathname === '/setup';
    // First run: no admin yet → force the operator through setup.
    if (!config.initialized && !onSetup) {
      redirect(307, '/setup');
    }
    // Already initialized: setup is closed → send stray visitors to login.
    if (config.initialized && onSetup) {
      redirect(307, '/login');
    }
  }

  return { user, config };
};
