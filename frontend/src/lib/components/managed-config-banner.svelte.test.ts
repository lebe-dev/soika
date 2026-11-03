// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import ManagedConfigBanner from './managed-config-banner.svelte';

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
});

function render(props: Record<string, unknown>) {
  setLocale('en');
  target = document.createElement('div');
  document.body.appendChild(target);
  flushSync(() => {
    component = mount(ManagedConfigBanner, { target, props });
  });
  return target.textContent ?? '';
}

describe('managed configuration banner', () => {
  it('stays hidden when everything is managed in the interface', () => {
    const text = render({
      projectSource: 'ui',
      teamSource: 'ui',
      serviceSource: 'ui',
      notificationSource: 'ui'
    });
    expect(text.trim()).toBe('');
  });

  it('lists every area that is managed by the deployment configuration', () => {
    const text = render({
      projectSource: 'yaml',
      teamSource: 'yaml',
      serviceSource: 'env',
      notificationSource: 'env'
    });
    expect(text).toContain('Managed by deployment configuration');
    expect(text).toContain('service settings and notifications, projects, teams');
    expect(text).not.toContain('MANAGEMENT_SOURCE');
  });

  it('mentions legacy YAML notifications on their own', () => {
    const text = render({
      projectSource: 'ui',
      teamSource: 'ui',
      serviceSource: 'ui',
      notificationSource: 'yaml'
    });
    expect(text).toContain('notifications');
    expect(text).not.toContain('service settings');
  });

  it('links "deployment configuration" to the deploy URL in a new tab', () => {
    render({
      projectSource: 'yaml',
      deployUrl: 'https://git.example.com/ops/soika'
    });
    const link = target.querySelector('a');
    expect(link?.textContent?.trim()).toBe('deployment configuration');
    expect(link?.getAttribute('href')).toBe('https://git.example.com/ops/soika');
    expect(link?.getAttribute('target')).toBe('_blank');
    expect(link?.getAttribute('rel')).toContain('noopener');
    expect(link?.querySelector('svg')).not.toBeNull();
  });

  it('renders plain text when no deploy URL is set', () => {
    const text = render({ projectSource: 'yaml', deployUrl: null });
    expect(target.querySelector('a')).toBeNull();
    expect(text).toContain('Managed by deployment configuration');
  });
});
