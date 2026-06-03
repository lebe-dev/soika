// Browser-side Sentry initialization.
//
// The DSN comes from the authenticated `GET /api/client-config` endpoint, so we
// only ever initialize Sentry after the operator has signed in (the DSN must
// not be exposed on a public route). Initialization is idempotent — the root
// layout's reactive effect may call it more than once across navigations.
//
// Errors only: we keep Sentry's default integrations (which capture uncaught
// errors and unhandled promise rejections) and do NOT add performance tracing.

import * as Sentry from '@sentry/svelte';
import type { ClientConfig } from '$lib/api';

let initialized = false;

/**
 * Initialize Sentry from the server-provided client config. No-op when already
 * initialized or when no DSN is configured (error reporting disabled).
 */
export function initSentry(config: ClientConfig | null | undefined): void {
  if (initialized || !config?.sentry_dsn) return;

  Sentry.init({
    dsn: config.sentry_dsn,
    environment: config.sentry_environment ?? undefined,
    release: config.release,
    // Errors only — disable performance tracing entirely.
    tracesSampleRate: 0
  });

  initialized = true;
}
