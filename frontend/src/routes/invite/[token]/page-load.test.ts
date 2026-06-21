// Tests for the accept-invite page load: it fetches the invite preview so the
// page can render the right flow. An invalid/expired token does not throw —
// the error is captured into `error` (via errorMessage) and `preview` stays
// null, letting the component render the failure state.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api/client';

vi.mock('$lib/api', async () => {
  const actual = await vi.importActual<typeof import('$lib/api')>('$lib/api');
  return {
    ...actual,
    invites: { preview: vi.fn() }
  };
});

import { invites } from '$lib/api';
import { load } from './+page';

const preview = vi.mocked(invites.preview);

function callLoad() {
  return (load as (e: unknown) => Promise<unknown>)({
    params: { token: 'tok123' },
    fetch: vi.fn()
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('invite/[token] +page load', () => {
  it('returns the preview and no error on success', async () => {
    const p = { team_name: 'Team', email: 'a@b.c' } as never;
    preview.mockResolvedValue(p);

    const data = (await callLoad()) as { token: string; preview: unknown; error: string | null };

    expect(data.token).toBe('tok123');
    expect(data.preview).toBe(p);
    expect(data.error).toBeNull();
  });

  it('captures a failed preview into `error` and leaves preview null', async () => {
    preview.mockRejectedValue(new ApiError(404, 'gone', { error: 'Invite expired' }));

    const data = (await callLoad()) as { token: string; preview: unknown; error: string | null };

    expect(data.token).toBe('tok123');
    expect(data.preview).toBeNull();
    expect(data.error).toBeTruthy();
  });
});
