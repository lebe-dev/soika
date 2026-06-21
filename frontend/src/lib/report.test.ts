// Tests for reportUnexpected: plain client errors (4xx ApiError) are
// intentionally NOT reported to Sentry (toast-only, avoids noise); non-ApiError
// failures and 5xx server errors are captured.

import { beforeEach, describe, expect, it, vi } from 'vitest';

const captureException = vi.fn();
vi.mock('@sentry/svelte', () => ({
  captureException: (...args: unknown[]) => captureException(...args)
}));

import { ApiError } from '$lib/api/client';
import { reportUnexpected } from './report';

beforeEach(() => {
  captureException.mockClear();
});

describe('reportUnexpected', () => {
  it('does not report a 4xx ApiError', () => {
    reportUnexpected(new ApiError(404, 'not found', null));
    expect(captureException).not.toHaveBeenCalled();
  });

  it('does not report a 400 ApiError', () => {
    reportUnexpected(new ApiError(400, 'bad request', null));
    expect(captureException).not.toHaveBeenCalled();
  });

  it('reports a 5xx ApiError', () => {
    const err = new ApiError(500, 'server error', null);
    reportUnexpected(err);
    expect(captureException).toHaveBeenCalledWith(err);
  });

  it('reports a non-ApiError failure', () => {
    const err = new TypeError('boom');
    reportUnexpected(err);
    expect(captureException).toHaveBeenCalledWith(err);
  });
});
