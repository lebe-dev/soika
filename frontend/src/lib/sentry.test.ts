import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ClientConfig } from '$lib/api';

// Spy on Sentry.init. `vi.mock` is hoisted, so the factory must not close over
// outer variables — we read the spy back via the mocked module after import.
vi.mock('@sentry/svelte', () => ({
  init: vi.fn(),
  setUser: vi.fn()
}));

// Import a fresh copy of the module (resets the module-level `initialized`
// flag) together with the mocked Sentry, so each call sees a clean spy.
async function load() {
  const Sentry = await import('@sentry/svelte');
  const { initSentry } = await import('./sentry');
  return { init: Sentry.init as ReturnType<typeof vi.fn>, initSentry };
}

beforeEach(() => {
  vi.resetModules();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('initSentry', () => {
  it('does not init when config is null', async () => {
    const { init, initSentry } = await load();
    initSentry(null);
    expect(init).not.toHaveBeenCalled();
  });

  it('does not init when config is undefined', async () => {
    const { init, initSentry } = await load();
    initSentry(undefined);
    expect(init).not.toHaveBeenCalled();
  });

  it('does not init when sentry_dsn is null', async () => {
    const { init, initSentry } = await load();
    const config: ClientConfig = {
      sentry_dsn: null,
      sentry_environment: null,
      release: 'soika@0.3.0',
      timezone: 'UTC'
    };
    initSentry(config);
    expect(init).not.toHaveBeenCalled();
  });

  it('does not init when sentry_dsn is an empty string', async () => {
    const { init, initSentry } = await load();
    const config: ClientConfig = {
      sentry_dsn: '',
      sentry_environment: 'production',
      release: 'soika@0.3.0',
      timezone: 'UTC'
    };
    initSentry(config);
    expect(init).not.toHaveBeenCalled();
  });

  it('inits once with dsn/environment/release and tracesSampleRate 0 when a dsn is present', async () => {
    const { init, initSentry } = await load();
    const config: ClientConfig = {
      sentry_dsn: 'https://public@sentry.example/42',
      sentry_environment: 'production',
      release: 'soika@0.3.0',
      timezone: 'UTC'
    };
    initSentry(config);
    expect(init).toHaveBeenCalledTimes(1);
    expect(init).toHaveBeenCalledWith({
      dsn: 'https://public@sentry.example/42',
      environment: 'production',
      release: 'soika@0.3.0',
      tracesSampleRate: 0
    });
  });

  it('passes environment as undefined when sentry_environment is null', async () => {
    const { init, initSentry } = await load();
    const config: ClientConfig = {
      sentry_dsn: 'https://public@sentry.example/42',
      sentry_environment: null,
      release: 'soika@0.3.0',
      timezone: 'UTC'
    };
    initSentry(config);
    expect(init).toHaveBeenCalledTimes(1);
    expect(init).toHaveBeenCalledWith({
      dsn: 'https://public@sentry.example/42',
      environment: undefined,
      release: 'soika@0.3.0',
      tracesSampleRate: 0
    });
  });

  it('is idempotent: a second call is a no-op', async () => {
    const { init, initSentry } = await load();
    const config: ClientConfig = {
      sentry_dsn: 'https://public@sentry.example/42',
      sentry_environment: 'production',
      release: 'soika@0.3.0',
      timezone: 'UTC'
    };
    initSentry(config);
    initSentry(config);
    expect(init).toHaveBeenCalledTimes(1);
  });
});
