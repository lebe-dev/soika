// Tests for the root +layout.ts load:
//   - bootstrap fetch memoization (module-level cachedConfig/lastUrl),
//   - graceful handling of a rejected auth.config (no cached rejection),
//   - first-run redirect routing (must NOT be swallowed by the catch).
//
// `cachedConfig`/`lastUrl` are module-level, so each case `vi.resetModules()`
// and re-imports a fresh copy of the module. `auth.config` and the `redirect`
// thrower from '@sveltejs/kit' are mocked so no real network/navigation runs.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AuthConfig } from '$lib/api';

// A thrown redirect in SvelteKit is an object carrying status + location; the
// real `redirect()` throws it. We mirror that so the layout's `redirect(...)`
// calls throw and our tests can inspect the thrown value.
class RedirectError extends Error {
  constructor(
    public status: number,
    public location: string
  ) {
    super(`redirect ${status} -> ${location}`);
  }
}

vi.mock('@sveltejs/kit', () => ({
  redirect: (status: number, location: string) => {
    throw new RedirectError(status, location);
  }
}));

// `auth.config` is the single bootstrap call. We mock the whole api barrel and
// drive the resolved/rejected value per case.
const configMock = vi.fn();
vi.mock('$lib/api', () => ({
  auth: {
    config: (...args: unknown[]) => configMock(...args)
  }
}));

// A minimal AuthConfig builder; only the fields the load reads matter.
function authConfig(overrides: Partial<AuthConfig> = {}): AuthConfig {
  return {
    oauth_enabled: false,
    oauth_provider_name: '',
    password_login_enabled: true,
    allow_signup: false,
    initialized: true,
    user: null,
    telemetry: null,
    projects: null,
    ...overrides
  } as AuthConfig;
}

// SvelteKit hands the load a `URL`; we only need `.href` and `.pathname`.
function makeUrl(href: string): URL {
  return new URL(href, 'http://localhost');
}

// Fresh import after a module reset so cachedConfig/lastUrl start null.
async function importLoad() {
  const mod = await import('./+layout');
  return mod.load;
}

const noopFetch = (() => Promise.resolve(new Response())) as unknown as typeof fetch;

// The data the load resolves with (when it does not throw a redirect). The
// declared `LayoutLoad` return type is a `void`-inclusive union, so we narrow
// to the concrete shape this load returns for ergonomic, type-safe assertions.
interface LoadData {
  user: AuthConfig['user'] | null;
  config: AuthConfig | null;
  telemetry: AuthConfig['telemetry'] | null;
}

async function call(load: Awaited<ReturnType<typeof importLoad>>, href: string): Promise<LoadData> {
  // The load only touches `fetch` and `url` from its event arg.
  const data = await load({ fetch: noopFetch, url: makeUrl(href) } as never);
  return data as LoadData;
}

beforeEach(() => {
  vi.resetModules();
  configMock.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('+layout load: bootstrap memoization', () => {
  it('refetches when the same url is loaded twice (initial load / invalidateAll)', async () => {
    configMock.mockResolvedValue(authConfig());
    const load = await importLoad();

    await call(load, '/login');
    await call(load, '/login');

    // First run: cachedConfig is null → fetch. Second run with the SAME href:
    // `sameUrl` is true → fetch again (this is the invalidateAll / reload path).
    expect(configMock).toHaveBeenCalledTimes(2);
  });

  it('reuses the cached promise across a url change, then refetches when that url repeats', async () => {
    configMock.mockResolvedValue(authConfig());
    const load = await importLoad();

    // Run 1 (/login): fetch (cachedConfig null).
    await call(load, '/login');
    expect(configMock).toHaveBeenCalledTimes(1);

    // Run 2 (/projects): url CHANGED → reuse the cached promise, no new fetch.
    await call(load, '/projects');
    expect(configMock).toHaveBeenCalledTimes(1);

    // Run 3 (/projects again): same url → refetch.
    await call(load, '/projects');
    expect(configMock).toHaveBeenCalledTimes(2);
  });

  it('forwards the request-scoped fetch to auth.config', async () => {
    configMock.mockResolvedValue(authConfig());
    const load = await importLoad();

    await call(load, '/login');

    expect(configMock).toHaveBeenCalledWith({ fetch: noopFetch });
  });
});

describe('+layout load: rejected bootstrap', () => {
  it('resolves with user:null/config:null/telemetry:null when auth.config rejects', async () => {
    configMock.mockRejectedValue(new Error('network down'));
    const load = await importLoad();

    const data = await call(load, '/login');

    expect(data).toEqual({ user: null, config: null, telemetry: null });
  });

  it('drops the rejected memo so the next run retries instead of replaying it', async () => {
    configMock.mockRejectedValueOnce(new Error('network down'));
    configMock.mockResolvedValue(authConfig({ user: { id: 'u1' } as never }));
    const load = await importLoad();

    // Run 1: rejects → cachedConfig is cleared back to null.
    const first = await call(load, '/login');
    expect(first.config).toBeNull();

    // Run 2 (url changed): because the memo was dropped, cachedConfig is null
    // again so it refetches rather than awaiting the stale rejected promise.
    const second = await call(load, '/projects');
    expect(configMock).toHaveBeenCalledTimes(2);
    expect(second.config).not.toBeNull();
    expect(second.user).toEqual({ id: 'u1' });
  });
});

describe('+layout load: first-run redirect routing', () => {
  it('redirects 307 -> /setup when uninitialized and not already on /setup', async () => {
    configMock.mockResolvedValue(authConfig({ initialized: false }));
    const load = await importLoad();

    await expect(call(load, '/login')).rejects.toMatchObject({
      status: 307,
      location: '/setup'
    });
  });

  it('does NOT redirect when uninitialized and already on /setup', async () => {
    configMock.mockResolvedValue(authConfig({ initialized: false }));
    const load = await importLoad();

    const data = await call(load, '/setup');

    expect(data.config?.initialized).toBe(false);
  });

  it('redirects 307 -> /login when initialized and on /setup', async () => {
    configMock.mockResolvedValue(authConfig({ initialized: true }));
    const load = await importLoad();

    await expect(call(load, '/setup')).rejects.toMatchObject({
      status: 307,
      location: '/login'
    });
  });

  it('does not redirect when initialized and off /setup', async () => {
    configMock.mockResolvedValue(authConfig({ initialized: true }));
    const load = await importLoad();

    const data = await call(load, '/login');

    expect(data.config?.initialized).toBe(true);
  });

  it('does not swallow the thrown redirect in the bootstrap catch', async () => {
    // config resolves fine (catch is not entered), then redirect throws and must
    // propagate out of load — the surrounding try/catch only guards the fetch.
    configMock.mockResolvedValue(authConfig({ initialized: false }));
    const load = await importLoad();

    await expect(call(load, '/login')).rejects.toBeInstanceOf(RedirectError);
  });
});
