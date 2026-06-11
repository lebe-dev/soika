// Browser-side Sentry initialization.
//
// The DSN comes from the authenticated bootstrap `GET /auth/config` endpoint
// (the `data.telemetry` slice), so we only ever initialize Sentry after the
// operator has signed in (the DSN must not be exposed on a public route).
// Initialization is idempotent — the root layout's reactive effect may call it
// more than once across navigations.
//
// Errors only: we keep Sentry's default integrations (which capture uncaught
// errors and unhandled promise rejections) and do NOT add performance tracing.

import * as Sentry from '@sentry/svelte';
import type { ClientConfig, User } from '$lib/api';

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

/**
 * Attach (or clear) the current user on the Sentry scope so captured events
 * carry `{ id, email }`. Pass `null`/`undefined` on logout to drop it. A no-op
 * when Sentry was never initialized (reporting disabled).
 */
export function setSentryUser(user: User | null | undefined): void {
  if (!initialized) return;
  if (!user) {
    Sentry.setUser(null);
    return;
  }
  Sentry.setUser({ id: user.id, email: user.email });
}
