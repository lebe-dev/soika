import { addMessages, init, locale, t } from 'svelte-intl-precompile';
import { get } from 'svelte/store';
import en from '$locales/en';
import ru from '$locales/ru';
import { detectLocale, type Locale } from './locale';

addMessages('en', en);
addMessages('ru', ru);
init({ fallbackLocale: 'en', initialLocale: 'en' });

export { locale, t };

export function browserLocale(): Locale {
  return detectLocale(typeof navigator === 'undefined' ? undefined : navigator.language);
}

export function setLocale(value: Locale) {
  locale.set(value);
  if (typeof document !== 'undefined') document.documentElement.lang = value;
}

/** For non-component code; components use the reactive $t store directly. */
export function translate(
  key: string,
  values?: Record<string, string | number | Date>,
  language?: string | null
): string {
  return get(t)(key, { values, ...(language ? { locale: language } : {}) });
}

export function currentLocale(): Locale {
  return get(locale) === 'ru' ? 'ru' : 'en';
}
