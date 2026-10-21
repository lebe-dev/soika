import { describe, expect, it } from 'vitest';
import { detectLocale } from './locale';

describe('browser language detection', () => {
  it.each(['ru', 'ru-RU', 'ru-BY', 'RU-ru', 'ru_RU'])('uses Russian for %s', (value) => {
    expect(detectLocale(value)).toBe('ru');
  });
  it.each(['en', 'en-US', 'de-DE', 'fr', '', null, undefined, 'russian'])(
    'defaults to English for %s',
    (value) => {
      expect(detectLocale(value)).toBe('en');
    }
  );
});
