import { describe, expect, it, vi } from 'vitest';
import { auth, invites, passkeys, teams } from './index';

function client() {
  const fetch = vi.fn(
    async (_url: RequestInfo | URL, _init?: RequestInit) =>
      new Response('{}', { headers: { 'content-type': 'application/json' } })
  );
  return { fetch };
}

describe('team deletion and invite URL serialization', () => {
  it('encodes the destination as a single query value and omits it when absent', async () => {
    const ctx = client();
    await teams.remove('old', 'new&extra=value', ctx);
    await teams.remove('empty', undefined, ctx);
    expect(ctx.fetch.mock.calls.map(([url]) => url)).toEqual([
      '/api/teams/old?move_projects_to=new%26extra%3Dvalue',
      '/api/teams/empty'
    ]);
    for (const [, init] of ctx.fetch.mock.calls) {
      expect(init).toMatchObject({ method: 'DELETE', credentials: 'include' });
    }
  });

  it('keeps reserved characters inside the invite token on preview, accept and revoke', async () => {
    const ctx = client();
    const token = 'token/with?reserved#characters';
    await invites.preview(token, ctx);
    await invites.accept(token, { display_name: 'Alice' }, ctx);
    await teams.revokeInvite('platform', token, ctx);
    const encoded = 'token%2Fwith%3Freserved%23characters';
    expect(ctx.fetch.mock.calls.map(([url]) => url)).toEqual([
      `/api/invite/${encoded}`,
      `/api/invite/${encoded}`,
      `/api/teams/platform/invites/${encoded}`
    ]);
    expect(ctx.fetch.mock.calls[1][1]).toMatchObject({
      method: 'POST',
      body: JSON.stringify({ display_name: 'Alice' })
    });
  });
});

describe('passkey ceremony requests', () => {
  it('sends a usernameless assertion to auth endpoints using the request-scoped fetch', async () => {
    const ctx = client();
    const assertion = { id: 'credential', response: { authenticatorData: 'opaque' } };
    await auth.passkeyLoginOptions(ctx);
    await auth.passkeyLogin(assertion, ctx);
    expect(ctx.fetch.mock.calls).toEqual([
      [
        '/auth/passkey/login/options',
        expect.objectContaining({ method: 'POST', credentials: 'include', body: undefined })
      ],
      [
        '/auth/passkey/login',
        expect.objectContaining({
          method: 'POST',
          credentials: 'include',
          body: JSON.stringify(assertion)
        })
      ]
    ]);
  });

  it('registers and manages credentials through authenticated API endpoints', async () => {
    const ctx = client();
    const credential = { name: 'Laptop', credential: { id: 'credential' } };
    await passkeys.registerOptions(ctx);
    await passkeys.register(credential, ctx);
    await passkeys.list(ctx);
    await passkeys.rename('credential', { name: 'Office' }, ctx);
    await passkeys.remove('credential', ctx);
    expect(ctx.fetch.mock.calls.map(([url, init]) => [url, init?.method])).toEqual([
      ['/api/passkeys/options', 'POST'],
      ['/api/passkeys', 'POST'],
      ['/api/passkeys', 'GET'],
      ['/api/passkeys/credential', 'PATCH'],
      ['/api/passkeys/credential', 'DELETE']
    ]);
    expect(ctx.fetch.mock.calls[1][1]?.body).toBe(JSON.stringify(credential));
    expect(ctx.fetch.mock.calls[3][1]?.body).toBe(JSON.stringify({ name: 'Office' }));
  });
});
