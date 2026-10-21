export type Locale = 'en' | 'ru';

/** Use the browser's primary language, including regional variants. */
export function detectLocale(language?: string | null): Locale {
  return language?.toLowerCase().split(/[-_]/)[0] === 'ru' ? 'ru' : 'en';
}
