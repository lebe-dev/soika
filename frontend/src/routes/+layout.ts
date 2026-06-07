// Root layout load: this is a session-cookie SPA hitting the Rust JSON API,
// so we disable SSR (the server has no cookie context at prerender time) and
// run as a single-page app with a static fallback document.
//
// A single bootstrap request (GET /auth/config) returns the public config plus
// the session slice (current user + telemetry), so the layout, nav, and
// per-route guards can rely on `data.user` / `data.config` without each route
// re-fetching. The config also drives first-run routing: an uninitialized
// instance is sent to `/setup`; once initialized, `/setup` bounces back to login.

import { redirect } from '@sveltejs/kit';
import { auth, type AuthConfig, type ClientConfig, type User } from '$lib/api';
import type { LayoutLoad } from './$types';

export const ssr = false;
export const prerender = false;

// This load reads `url` for first-run routing, so SvelteKit reruns it on every
// navigation AND on preload-on-hover (see `data-sveltekit-preload-data` in
// app.html). The bootstrap config is URL-independent, so refetching it on each
// rerun would fire GET /auth/config many times per page. We memoize the fetch
// and only re-issue it when the URL did NOT change between runs — i.e. on the
// initial load or an explicit `invalidateAll()` after a mutation — reusing the
// cached promise for the route-to-route (and preload) reruns.
let cachedConfig: Promise<AuthConfig> | null = null;
let lastUrl: string | null = null;

export const load: LayoutLoad = async ({ fetch, url }) => {
  let user: User | null = null;
  let config: AuthConfig | null = null;
  let telemetry: ClientConfig | null = null;

  const sameUrl = lastUrl === url.href;
  lastUrl = url.href;
  if (!cachedConfig || sameUrl) {
    cachedConfig = auth.config({ fetch });
  }

  // Single bootstrap fetch; tolerate failures so a backend hiccup doesn't trap
  // the user. Redirects are decided AFTER this block — `redirect()` throws, and
  // we must not swallow it inside the catch.
  try {
    config = await cachedConfig;
    // `user`/`telemetry` are embedded for an authenticated session and `null`
    // for an anonymous one — route guards handle the auth redirects.
    user = config.user;
    telemetry = config.telemetry;
  } catch {
    // Network/server error: treat as unauthenticated with unknown config; route
    // guards handle auth redirects and we skip first-run routing below. Drop the
    // memo so a later run retries instead of replaying the rejection.
    cachedConfig = null;
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

  return { user, config, telemetry };
};
