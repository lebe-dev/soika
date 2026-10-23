// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { writable, type Readable } from 'svelte/store';
import { setLocale } from '$lib/i18n';
import Login from './+page.svelte';

const { page, supported } = vi.hoisted(() => ({
  page: { current: null as unknown },
  supported: vi.fn(() => true)
}));
vi.mock('$app/stores', () => ({
  page: {
    subscribe: (run: (value: unknown) => void) => (page.current as Readable<unknown>).subscribe(run)
  }
}));
vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$lib/passkey', () => ({
  passkeysSupported: supported,
  isCeremonyCancelled: vi.fn(),
  signInWithPasskey: vi.fn()
}));
vi.mock('$lib/components/ui/sonner', () => ({ toast: { error: vi.fn() } }));

let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
function render(passwordEnabled: boolean, sso: boolean, passkey: boolean) {
  page.current = writable({
    url: new URL('https://test.home/login'),
    data: {
      config: {
        password_login_enabled: passwordEnabled,
        oauth_enabled: sso,
        oauth_provider_name: 'GitLab',
        passkey_enabled: passkey,
        allow_signup: true
      }
    }
  });
  flushSync(() => {
    component = mount(Login, { target });
  });
}
beforeEach(() => {
  setLocale('en');
  target = document.createElement('div');
  document.body.appendChild(target);
});
afterEach(async () => {
  await unmount(component);
  target.remove();
});

describe('email login visibility', () => {
  it('keeps the password form alongside SSO when enabled', () => {
    render(true, true, false);
    expect(target.querySelector('#email')).not.toBeNull();
    expect(target.querySelector('#password')).not.toBeNull();
    expect(target.querySelector('button[type="submit"]')).not.toBeNull();
    expect(target.textContent?.replace(/\s+/g, ' ')).toContain('Sign in with GitLab');
  });
  it('shows only SSO without password fields, signup or a leading separator', () => {
    render(false, true, false);
    expect(target.querySelector('input')).toBeNull();
    expect(target.querySelector('button[type="submit"]')).toBeNull();
    expect(target.querySelector('a[href="/register"]')).toBeNull();
    expect(target.textContent?.replace(/\s+/g, ' ')).toContain('Sign in with GitLab');
    expect(target.querySelectorAll('.uppercase')).toHaveLength(0);
  });
  it('keeps passkeys available and separates them from SSO', () => {
    render(false, true, true);
    expect(target.querySelector('input')).toBeNull();
    expect(target.textContent).toContain('Sign in with a passkey');
    expect(target.textContent?.replace(/\s+/g, ' ')).toContain('Sign in with GitLab');
    expect(target.querySelectorAll('.uppercase')).toHaveLength(1);
  });
  it('offers passkey sign-in without a leading separator when SSO is off', () => {
    render(false, false, true);
    expect(target.querySelector('input')).toBeNull();
    expect(target.textContent).toContain('Sign in with a passkey');
    expect(target.querySelectorAll('.uppercase')).toHaveLength(0);
  });
});
