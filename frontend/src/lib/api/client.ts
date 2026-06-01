// Low-level fetch client for the soika JSON API (MVP §16).
//
// Talks to the Rust backend on the SAME ORIGIN as the SPA (the binary serves
// both), so the opaque, signed session cookie is sent automatically. We still
// pass `credentials: 'include'` so the dev proxy (vite.config.ts) and any
// cross-origin previews keep the cookie flowing.
//
// In SvelteKit `load` functions a request-scoped `fetch` should be forwarded so
// SSR/relative URLs resolve and cookies pass through; every helper accepts an
// optional `fetch` for that reason.

import type { ApiErrorBody } from './types';

export type Fetch = typeof fetch;

/** A typed error carrying the backend's HTTP status and `{ error }` message. */
export class ApiError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, message: string, body: unknown) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.body = body;
  }

  get isUnauthorized() {
    return this.status === 401;
  }

  get isForbidden() {
    return this.status === 403;
  }

  get isNotFound() {
    return this.status === 404;
  }
}

/**
 * The user-facing message for a failed request: the backend's `{ error }` text
 * when the failure is an {@link ApiError}, otherwise the given `fallback`.
 * Centralizes the `err instanceof ApiError ? err.message : fallback` idiom used
 * by every form/action handler and `load`.
 */
export function errorMessage(err: unknown, fallback: string): string {
  return err instanceof ApiError ? err.message : fallback;
}

interface RequestOptions {
  /** Request-scoped fetch (forward SvelteKit's `fetch` inside `load`). */
  fetch?: Fetch;
  /** JSON body; serialized automatically with the right content-type. */
  body?: unknown;
  /**
   * Query string parameters; `undefined`/`null` values are dropped. Accepts any
   * object (typed query DTOs included) — values are coerced to strings.
   */
  query?: Record<string, unknown> | object;
  signal?: AbortSignal;
  headers?: Record<string, string>;
}

function buildUrl(path: string, query?: RequestOptions['query']): string {
  if (!query) return path;
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null) continue;
    params.set(key, String(value));
  }
  const qs = params.toString();
  return qs ? `${path}?${qs}` : path;
}

async function parseError(res: Response): Promise<ApiError> {
  let body: unknown = undefined;
  let message = `request failed with status ${res.status}`;
  try {
    body = await res.json();
    const err = (body as ApiErrorBody | undefined)?.error;
    if (typeof err === 'string' && err.length > 0) message = err;
  } catch {
    // Non-JSON error body (e.g. a plain-text 401 from the auth extractor).
    try {
      const text = await res.text();
      if (text) message = text;
    } catch {
      // ignore — keep the default message
    }
  }
  return new ApiError(res.status, message, body);
}

async function request<T>(method: string, path: string, options: RequestOptions = {}): Promise<T> {
  const doFetch = options.fetch ?? fetch;
  const headers: Record<string, string> = { ...options.headers };

  let body: BodyInit | undefined;
  if (options.body !== undefined) {
    headers['content-type'] = 'application/json';
    body = JSON.stringify(options.body);
  }

  const res = await doFetch(buildUrl(path, options.query), {
    method,
    headers,
    body,
    credentials: 'include',
    signal: options.signal
  });

  if (!res.ok) throw await parseError(res);

  // 204 No Content (logout, deletes) and empty bodies return undefined.
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

export const http = {
  get: <T>(path: string, options?: RequestOptions) => request<T>('GET', path, options),
  post: <T>(path: string, options?: RequestOptions) => request<T>('POST', path, options),
  patch: <T>(path: string, options?: RequestOptions) => request<T>('PATCH', path, options),
  put: <T>(path: string, options?: RequestOptions) => request<T>('PUT', path, options),
  delete: <T>(path: string, options?: RequestOptions) => request<T>('DELETE', path, options)
};
