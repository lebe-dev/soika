import { describe, expect, it } from 'vitest';
import { extractContext, isContextEmpty } from './event-context';

describe('extractContext', () => {
  it('extracts an empty context from a bare payload', () => {
    const ctx = extractContext({ message: 'hi' });
    expect(isContextEmpty(ctx)).toBe(true);
  });

  it('surfaces browser, OS, IP, environment and release as highlights', () => {
    const ctx = extractContext({
      environment: 'production',
      release: 'web@1.2.3',
      server_name: 'edge-1',
      user: { id: '42', email: 'a@b.c', ip_address: '203.0.113.7' },
      contexts: {
        browser: { type: 'browser', name: 'Chrome', version: '120.0.0.0' },
        os: { type: 'os', name: 'Windows', version: '10' }
      }
    });

    const highlights = Object.fromEntries(ctx.highlights.map((h) => [h.key, h.value]));
    expect(highlights.Browser).toBe('Chrome 120.0.0.0');
    expect(highlights.OS).toBe('Windows 10');
    expect(highlights.IP).toBe('203.0.113.7');
    expect(highlights.Environment).toBe('production');
    expect(highlights.Release).toBe('web@1.2.3');
    expect(highlights.Server).toBe('edge-1');
  });

  it('lists user identity fields in order', () => {
    const ctx = extractContext({
      user: { username: 'neo', email: 'neo@zion.io', id: 'u1' }
    });
    expect(ctx.user).toEqual([
      { key: 'ID', value: 'u1' },
      { key: 'Username', value: 'neo' },
      { key: 'Email', value: 'neo@zion.io' }
    ]);
  });

  it('normalizes object-shaped tags', () => {
    const ctx = extractContext({ tags: { level: 'error', browser: 'Chrome', empty: '' } });
    expect(ctx.tags).toEqual([
      { key: 'level', value: 'error' },
      { key: 'browser', value: 'Chrome' }
    ]);
  });

  it('normalizes array-shaped tags', () => {
    const ctx = extractContext({
      tags: [['runtime', 'node'], { key: 'shard', value: '3' }]
    });
    expect(ctx.tags).toEqual([
      { key: 'runtime', value: 'node' },
      { key: 'shard', value: '3' }
    ]);
  });

  it('extracts request url, method and selected headers', () => {
    const ctx = extractContext({
      request: {
        url: 'https://app.example.com/dashboard',
        method: 'POST',
        query_string: 'page=2',
        headers: {
          'User-Agent': 'Mozilla/5.0',
          Referer: 'https://app.example.com/',
          'X-Internal': 'ignored'
        }
      }
    });
    expect(ctx.request?.url).toBe('https://app.example.com/dashboard');
    expect(ctx.request?.method).toBe('POST');
    expect(ctx.request?.query).toBe('page=2');
    expect(ctx.request?.headers).toEqual([
      { key: 'User-Agent', value: 'Mozilla/5.0' },
      { key: 'Referer', value: 'https://app.example.com/' }
    ]);
  });

  it('combines product and generic contexts', () => {
    const ctx = extractContext({
      contexts: {
        browser: { name: 'Firefox', version: '121.0' },
        runtime: { name: 'browser' },
        app: { app_name: 'demo', build_type: 'release' }
      }
    });
    const labels = ctx.contexts.map((c) => c.label);
    expect(labels).toContain('Browser');
    expect(labels).toContain('Runtime');
  });

  it('keeps generic contexts as detail sections with their fields', () => {
    const ctx = extractContext({
      contexts: {
        'Rust Tracing Fields': {
          type: 'unknown',
          error: 'database error: (code: 5) database is locked',
          project_id: '502f5798'
        },
        browser: { name: 'Firefox', version: '121.0' }
      }
    });

    const fields = ctx.details.find((d) => d.label === 'Rust Tracing Fields');
    expect(fields?.rows).toEqual([
      { key: 'error', value: 'database error: (code: 5) database is locked' },
      { key: 'project_id', value: '502f5798' }
    ]);
    // Product contexts stay in the compact `contexts` list, not duplicated here.
    expect(ctx.details.map((d) => d.label)).not.toContain('Browser');
  });

  it('flattens the nested `data` object of a trace context', () => {
    const ctx = extractContext({
      contexts: {
        trace: {
          type: 'trace',
          op: 'soika::router::http',
          trace_id: 'ccd5f410',
          data: { method: 'POST', path: '/api/x/envelope/', 'code.line.number': 170 }
        }
      }
    });

    const trace = ctx.details.find((d) => d.label === 'Trace');
    const rows = Object.fromEntries(trace!.rows.map((r) => [r.key, r.value]));
    expect(rows.op).toBe('soika::router::http');
    expect(rows.method).toBe('POST');
    expect(rows.path).toBe('/api/x/envelope/');
    expect(rows['code.line.number']).toBe('170');
    expect(rows.type).toBeUndefined();
  });

  it('surfaces a tracing error field as a headline chip', () => {
    const ctx = extractContext({
      contexts: {
        'Rust Tracing Fields': { type: 'unknown', error: 'database is locked' }
      }
    });
    expect(ctx.error).toBe('database is locked');
  });

  it('reports a context-only payload as non-empty', () => {
    const ctx = extractContext({
      contexts: { 'Rust Tracing Location': { file: 'src/ingest/handlers.rs', line: 84 } }
    });
    expect(isContextEmpty(ctx)).toBe(false);
  });

  it('joins SDK name and version', () => {
    const ctx = extractContext({ sdk: { name: 'sentry.javascript.svelte', version: '8.0.0' } });
    expect(ctx.sdk).toBe('sentry.javascript.svelte 8.0.0');
  });

  it('stringifies additional (extra) data', () => {
    const ctx = extractContext({ extra: { count: 3, note: 'hi', meta: { a: 1 } } });
    expect(ctx.additional).toEqual([
      { key: 'count', value: '3' },
      { key: 'note', value: 'hi' },
      { key: 'meta', value: '{"a":1}' }
    ]);
  });
});
