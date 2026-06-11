import * as Sentry from '@sentry/svelte';
import { ApiError } from '$lib/api/client';

/**
 * Report an unexpected error to Sentry.
 *
 * Plain client errors (4xx `ApiError`s) are toast-only and intentionally NOT
 * reported, to avoid noise. Everything else — non-`ApiError` failures and 5xx
 * server errors — is captured. Safe to call when Sentry is uninitialized:
 * `captureException` is a no-op in that case.
 */
export function reportUnexpected(err: unknown): void {
  if (err instanceof ApiError && err.status < 500) return;
  Sentry.captureException(err);
}
