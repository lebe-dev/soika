// Client-side error hook. SvelteKit calls `handleError` for uncaught
// load/render errors; reporting them here routes those failures to Sentry with
// the active route attached. Sentry's `init` lives in `$lib/sentry` (gated on
// the authenticated client config) — we only capture, never initialize, so
// nothing is sent until the SDK has been initialized post sign-in.
import * as Sentry from '@sentry/svelte';
import type { HandleClientError } from '@sveltejs/kit';

export const handleError: HandleClientError = ({ error }) => {
  Sentry.captureException(error);

  return {
    message: 'An unexpected error occurred.'
  };
};
