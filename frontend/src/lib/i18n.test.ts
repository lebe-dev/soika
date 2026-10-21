import { afterEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { readFileSync } from 'node:fs';
import { currentLocale, setLocale, t, translate } from './i18n';
import { formatCount, formatDateTime, formatRelative } from './format';
import { ApiError, errorMessage } from './api/client';

const catalog = (language: string): Record<string, string> =>
  JSON.parse(readFileSync(new URL(`../../locales/${language}.json`, import.meta.url), 'utf8'));

afterEach(() => setLocale('en'));

describe('compiled English/Russian catalogs', () => {
  it('ships a Russian translation for every English message', () => {
    const en = catalog('en');
    const ru = catalog('ru');
    expect(Object.keys(ru).sort()).toEqual(Object.keys(en).sort());
    for (const [key, value] of Object.entries(ru)) {
      expect(value.trim(), key).not.toBe('');
    }
  });

  it('reactively updates the formatter store when the language changes', () => {
    const values: string[] = [];
    const unsubscribe = t.subscribe((format) => values.push(format('profile')));
    setLocale('ru');
    expect(currentLocale()).toBe('ru');
    expect(get(t)('profile')).toBe('Профиль');
    setLocale('en');
    expect(values).toContain('Профиль');
    expect(values.at(-1)).toBe('Profile');
    unsubscribe();
  });

  it('compiles Russian ICU plural rules and interpolates names', () => {
    setLocale('ru');
    expect(translate('project_count', { count: 1 })).toBe('1 проект');
    expect(translate('project_count', { count: 2 })).toBe('2 проекта');
    expect(translate('project_count', { count: 5 })).toBe('5 проектов');
    expect(translate('project_count', { count: 21 })).toBe('21 проект');
    expect(translate('team_created', { name: 'Backend' })).toBe('Команда «Backend» создана');
  });

  it('formats dates, relative times, and counts using the selected locale', () => {
    setLocale('ru');
    expect(formatDateTime('2026-05-31T12:34:56Z', 'UTC')).toContain('мая');
    expect(formatRelative(new Date(Date.now() - 5 * 60_000).toISOString())).toBe('5 мин. назад');
    expect(formatCount(1500)).toContain('тыс.');
    setLocale('en');
    expect(formatDateTime('2026-05-31T12:34:56Z', 'UTC')).toContain('May');
  });

  it('translates known API validation errors and retains unknown diagnostic details', () => {
    setLocale('ru');
    expect(
      errorMessage(
        new ApiError(401, 'authentication error: invalid email or password', {}),
        'fallback'
      )
    ).toBe('Неверная почта или пароль');
    expect(errorMessage(new ApiError(500, 'unexpected diagnostic', {}), 'fallback')).toBe(
      'unexpected diagnostic'
    );
  });
});
