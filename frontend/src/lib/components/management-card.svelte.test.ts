// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { setLocale } from '$lib/i18n';
import type { ManagementStatus } from '$lib/api';
import ManagementCard from './management-card.svelte';
let component: ReturnType<typeof mount>;
let target: HTMLDivElement;
afterEach(async () => {
  await unmount(component);
  target.remove();
});
describe('management status', () => {
  for (const project of ['ui', 'yaml'] as const)
    for (const team of ['ui', 'yaml'] as const) {
      it(`explains ${project}/${team} management`, () => {
        setLocale('en');
        target = document.createElement('div');
        document.body.appendChild(target);
        const management: ManagementStatus = {
          settings: {
            path: '/etc/soika/config.yaml',
            project_source: project,
            team_source: team,
            notification_source: 'ui',
            project_orphan_policy: 'readonly',
            team_orphan_policy: 'readonly'
          },
          report: {
            loaded: project === 'yaml' || team === 'yaml',
            default_file: false,
            synced_at: null,
            created: 2,
            updated: 0,
            unchanged: 0,
            restored: 0,
            orphaned: 1,
            deleted: 0,
            projects_in_file: 1,
            teams_in_file: 1,
            pending_grants: 2,
            local_grants: 0
          },
          oauth_enabled: false
        };
        flushSync(() => {
          component = mount(ManagementCard, { target, props: { management } });
        });
        expect(target.textContent).toContain('PROJECT_MANAGEMENT_SOURCE');
        expect(target.textContent).toContain('/etc/soika/config.yaml');
        expect(target.textContent?.includes('new users cannot join')).toBe(team === 'yaml');
        expect(target.textContent).toContain(
          project === 'ui' && team === 'ui'
            ? 'Projects and teams are managed in the UI.'
            : 'configuration file'
        );
      });
    }
});
