// Tests for the root +layout.ts load:
//   - the URL is read untracked, so client-side navigation never reruns it
//     (only an explicit invalidation does) and every run fetches fresh config,
//   - graceful handling of a rejected auth.config,
//   - first-run redirect routing (must NOT be swallowed by the catch).
//
// Each case `vi.resetModules()` and re-imports a fresh copy of the module.
// `auth.config` and the `redirect` thrower from '@sveltejs/kit' are mocked so
// no real network/navigation runs.

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

// Fresh import after a module reset.
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

// How many times the load read `url` outside of `untrack` — a tracked read
// would make SvelteKit rerun the load (and refetch) on every navigation.
let trackedUrlReads = 0;

async function call(load: Awaited<ReturnType<typeof importLoad>>, href: string): Promise<LoadData> {
  const url = makeUrl(href);
  let untracked = false;
  const event = {
    fetch: noopFetch,
    // Like SvelteKit, a read of any URL property is what registers the
    // dependency, so the proxy counts reads made outside `untrack`.
    url: new Proxy(url, {
      get(target, prop) {
        if (!untracked) trackedUrlReads += 1;
        return Reflect.get(target, prop, target);
      }
    }),
    untrack<T>(fn: () => T): T {
      untracked = true;
      try {
        return fn();
      } finally {
        untracked = false;
      }
    }
  };
  const data = await load(event as never);
  return data as LoadData;
}

beforeEach(() => {
  vi.resetModules();
  configMock.mockReset();
  trackedUrlReads = 0;
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('+layout load: bootstrap fetch', () => {
  it('never reads the url as a tracked dependency', async () => {
    configMock.mockResolvedValue(authConfig());
    const load = await importLoad();

    await call(load, '/projects');

    expect(trackedUrlReads).toBe(0);
  });

  it('fetches fresh config on every run (initial load / invalidation)', async () => {
    configMock
      .mockResolvedValueOnce(authConfig({ user: null }))
      .mockResolvedValueOnce(authConfig({ user: { id: 'u1' } as never }));
    const load = await importLoad();

    // Signed out on /login, then the login flow invalidates and navigates to
    // `/`: the second run must see the new session, not a cached anonymous one.
    const before = await call(load, '/login');
    const after = await call(load, '/');

    expect(configMock).toHaveBeenCalledTimes(2);
    expect(before.user).toBeNull();
    expect(after.user).toEqual({ id: 'u1' });
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

  it('retries on the next run instead of replaying a failure', async () => {
    configMock.mockRejectedValueOnce(new Error('network down'));
    configMock.mockResolvedValue(authConfig({ user: { id: 'u1' } as never }));
    const load = await importLoad();

    const first = await call(load, '/login');
    expect(first.config).toBeNull();

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
