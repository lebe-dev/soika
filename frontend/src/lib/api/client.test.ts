import { describe, expect, it, vi } from 'vitest';
import { ApiError, errorMessage, http, type Fetch } from './client';

// Builds a minimal `Response`-like object for the fake fetch. `client.ts`
// only touches `ok`, `status`, `json()`, and `text()`, so we stub exactly
// those — `json` and `text` are separate jest fns because `parseError`
// falls back from `json()` to `text()` when the body is not JSON.
function fakeResponse(opts: {
  ok?: boolean;
  status: number;
  json?: () => Promise<unknown>;
  text?: () => Promise<string>;
}): Response {
  return {
    ok: opts.ok ?? (opts.status >= 200 && opts.status < 300),
    status: opts.status,
    json: opts.json ?? (async () => ({})),
    text: opts.text ?? (async () => '')
  } as unknown as Response;
}

/** A fake fetch that captures its arguments and returns the given response. */
function stubFetch(res: Response) {
  return vi.fn(
    async (_input: RequestInfo | URL, _init?: RequestInit) => res
  ) as unknown as Fetch & {
    mock: { calls: [string, RequestInit][] };
  };
}

/** Awaits a rejecting request and returns the caught error typed as ApiError. */
async function caughtError(p: Promise<unknown>): Promise<ApiError> {
  return p.then(
    () => {
      throw new Error('expected the request to reject, but it resolved');
    },
    (e) => e as ApiError
  );
}

describe('ApiError', () => {
  it('carries status, message, and body', () => {
    const body = { error: 'boom', extra: 1 };
    const err = new ApiError(418, 'boom', body);
    expect(err).toBeInstanceOf(Error);
    expect(err.name).toBe('ApiError');
    expect(err.status).toBe(418);
    expect(err.message).toBe('boom');
    expect(err.body).toBe(body);
  });

  it('exposes isUnauthorized / isForbidden / isNotFound by status', () => {
    expect(new ApiError(401, 'x', null).isUnauthorized).toBe(true);
    expect(new ApiError(401, 'x', null).isForbidden).toBe(false);
    expect(new ApiError(401, 'x', null).isNotFound).toBe(false);

    expect(new ApiError(403, 'x', null).isForbidden).toBe(true);
    expect(new ApiError(403, 'x', null).isUnauthorized).toBe(false);

    expect(new ApiError(404, 'x', null).isNotFound).toBe(true);
    expect(new ApiError(404, 'x', null).isForbidden).toBe(false);

    const ok = new ApiError(500, 'x', null);
    expect(ok.isUnauthorized).toBe(false);
    expect(ok.isForbidden).toBe(false);
    expect(ok.isNotFound).toBe(false);
  });
});

describe('errorMessage', () => {
  it('returns the ApiError message for an ApiError', () => {
    expect(errorMessage(new ApiError(400, 'bad input', null), 'fallback')).toBe('bad input');
  });

  it('returns the fallback for anything that is not an ApiError', () => {
    expect(errorMessage(new Error('native'), 'fallback')).toBe('fallback');
    expect(errorMessage('a string', 'fallback')).toBe('fallback');
    expect(errorMessage(undefined, 'fallback')).toBe('fallback');
    expect(errorMessage({ message: 'looks like one' }, 'fallback')).toBe('fallback');
  });
});

describe('request — error mapping (parseError)', () => {
  it('throws ApiError with status + message + body for a JSON { error }', async () => {
    const body = { error: 'team not found' };
    const fetchFn = stubFetch(fakeResponse({ status: 404, json: async () => body }));

    const err = await caughtError(http.get('/teams/abc', { fetch: fetchFn }));
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(404);
    expect(err.message).toBe('team not found');
    expect(err.body).toEqual(body);
    expect(err.isNotFound).toBe(true);
  });

  it('falls back to res.text() for a non-JSON error body', async () => {
    const fetchFn = stubFetch(
      fakeResponse({
        status: 401,
        json: async () => {
          throw new SyntaxError('not json');
        },
        text: async () => 'Unauthorized'
      })
    );

    const err = await caughtError(http.get('/issues', { fetch: fetchFn }));
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(401);
    expect(err.message).toBe('Unauthorized');
    // body stays undefined because json() threw before assignment.
    expect(err.body).toBeUndefined();
    expect(err.isUnauthorized).toBe(true);
  });

  it('falls back to "request failed with status N" when text is empty', async () => {
    const fetchFn = stubFetch(
      fakeResponse({
        status: 401,
        json: async () => {
          throw new SyntaxError('not json');
        },
        text: async () => ''
      })
    );

    const err = await caughtError(http.get('/issues', { fetch: fetchFn }));
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(401);
    expect(err.message).toBe('request failed with status 401');
    expect(err.body).toBeUndefined();
  });

  it('keeps the default message when a JSON body has no usable error field', async () => {
    const fetchFn = stubFetch(fakeResponse({ status: 500, json: async () => ({ error: '' }) }));

    const err = await caughtError(http.get('/issues', { fetch: fetchFn }));
    expect(err).toBeInstanceOf(ApiError);
    expect(err.message).toBe('request failed with status 500');
    expect(err.body).toEqual({ error: '' });
  });
});

describe('request — success bodies', () => {
  it('parses a JSON body on a 200', async () => {
    const fetchFn = stubFetch(fakeResponse({ status: 200, text: async () => '{"id":"x","n":2}' }));
    const out = await http.get<{ id: string; n: number }>('/me', { fetch: fetchFn });
    expect(out).toEqual({ id: 'x', n: 2 });
  });

  it('resolves to undefined for a 204 No Content', async () => {
    const fetchFn = stubFetch(
      fakeResponse({
        status: 204,
        text: async () => {
          throw new Error('text() must not be called for a 204');
        }
      })
    );
    const out = await http.delete('/issues/1', { fetch: fetchFn });
    expect(out).toBeUndefined();
  });

  it('resolves to undefined for an empty 200 body', async () => {
    const fetchFn = stubFetch(fakeResponse({ status: 200, text: async () => '' }));
    const out = await http.post('/logout', { fetch: fetchFn });
    expect(out).toBeUndefined();
  });
});

describe('buildUrl / withBase', () => {
  // buildUrl is internal; assert it through the URL passed to the fake fetch.
  async function capturedUrl(
    call: (fetchFn: Fetch) => Promise<unknown>
  ): Promise<{ url: string; init: RequestInit }> {
    const fetchFn = stubFetch(fakeResponse({ status: 200, text: async () => '' }));
    await call(fetchFn);
    const [url, init] = (fetchFn as unknown as { mock: { calls: [string, RequestInit][] } }).mock
      .calls[0];
    return { url, init };
  }

  it('prepends /api to a normal API path', async () => {
    const { url } = await capturedUrl((f) => http.get('/teams', { fetch: f }));
    expect(url).toBe('/api/teams');
  });

  it('leaves /auth/* paths at the root (no /api prefix)', async () => {
    const { url } = await capturedUrl((f) => http.get('/auth/config', { fetch: f }));
    expect(url).toBe('/auth/config');
  });

  it('appends a query string and drops null/undefined keys', async () => {
    const { url } = await capturedUrl((f) =>
      http.get('/issues', {
        fetch: f,
        query: { status: 'unresolved', page: 2, cursor: undefined, before: null }
      })
    );
    expect(url).toBe('/api/issues?status=unresolved&page=2');
  });

  it('returns the bare path when query is omitted', async () => {
    const { url } = await capturedUrl((f) => http.get('/issues', { fetch: f }));
    expect(url).toBe('/api/issues');
  });

  it('returns the bare path when every query value is dropped', async () => {
    const { url } = await capturedUrl((f) =>
      http.get('/issues', { fetch: f, query: { a: undefined, b: null } })
    );
    expect(url).toBe('/api/issues');
  });
});

describe('request — fetch init', () => {
  async function capturedInit(call: (fetchFn: Fetch) => Promise<unknown>): Promise<RequestInit> {
    const fetchFn = stubFetch(fakeResponse({ status: 200, text: async () => '' }));
    await call(fetchFn);
    return (fetchFn as unknown as { mock: { calls: [string, RequestInit][] } }).mock.calls[0][1];
  }

  it("always sends credentials: 'include'", async () => {
    const init = await capturedInit((f) => http.get('/me', { fetch: f }));
    expect(init.credentials).toBe('include');
  });

  it('omits content-type when there is no body', async () => {
    const init = await capturedInit((f) => http.get('/me', { fetch: f }));
    expect(init.method).toBe('GET');
    expect(init.body).toBeUndefined();
    expect((init.headers as Record<string, string>)['content-type']).toBeUndefined();
  });

  it('sets content-type and serializes the body to JSON when a body is sent', async () => {
    const init = await capturedInit((f) =>
      http.post('/teams', { fetch: f, body: { name: 'core' } })
    );
    expect(init.method).toBe('POST');
    expect((init.headers as Record<string, string>)['content-type']).toBe('application/json');
    expect(init.body).toBe(JSON.stringify({ name: 'core' }));
  });

  it('forwards custom headers and the abort signal', async () => {
    const controller = new AbortController();
    const init = await capturedInit((f) =>
      http.get('/me', { fetch: f, headers: { 'x-test': '1' }, signal: controller.signal })
    );
    expect((init.headers as Record<string, string>)['x-test']).toBe('1');
    expect(init.signal).toBe(controller.signal);
  });
});
