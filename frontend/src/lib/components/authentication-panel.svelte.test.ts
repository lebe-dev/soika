// @vitest-environment jsdom
import { afterEach, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import type { AuthenticationStatus } from '$lib/api';
import Panel from './authentication-panel.svelte';
let component: ReturnType<typeof mount>;
const target = document.createElement('div');
afterEach(async () => {
  await unmount(component);
  target.replaceChildren();
  setLocale('en');
});
const base: AuthenticationStatus = {
  password_login: 'all',
  sso: null,
  passkeys: null,
  public_signup: true,
  yaml_teams: false,
  admin_setup_enabled: true,
  login_protection_enabled: true
};
function render(authentication: AuthenticationStatus) {
  flushSync(() => {
    component = mount(Panel, { target, props: { authentication } });
  });
  return target.textContent!;
}
it('shows password access and disabled alternatives without provider details', () => {
  const text = render(base);
  expect(text).toContain('Password sign-in is available to local accounts.');
  expect(text).toContain('No SSO provider is enabled');
  expect(text).toContain('Passkey registration and sign-in are disabled.');
  expect(text).not.toContain('Allowed email domains');
});
it('shows SSO, domain restrictions, passkeys and YAML policy', () => {
  const text = render({
    ...base,
    password_login: 'owners',
    yaml_teams: true,
    public_signup: false,
    sso: {
      provider_name: 'GitLab',
      allowed_email_domains: ['example.com'],
      require_approval: true
    },
    passkeys: {
      rp_id: 'errors.example.com',
      rp_origin: 'https://errors.example.com',
      max_per_user: 5
    }
  });
  expect(text).toContain('GitLab');
  expect(text).toContain('example.com');
  expect(text).toContain('Required');
  expect(text).toContain('limited to local Owners');
  expect(text).toContain('Teams are managed through YAML');
  expect(text).toContain('https://errors.example.com');
});
it('localizes SSO and disabled passwords in Russian', () => {
  setLocale('ru');
  const text = render({
    ...base,
    password_login: 'disabled',
    sso: { provider_name: 'GitHub', allowed_email_domains: [], require_approval: false }
  });
  expect(text).toContain('Единый вход (SSO)');
  expect(text).toContain('Любой домен');
  expect(text).toContain('Не требуется');
  expect(text).toContain('Вход по паролю отключён для всех аккаунтов');
});
