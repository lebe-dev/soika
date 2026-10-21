import { currentLocale, translate } from './i18n';
// Small display formatting helpers shared by the project/issue pages.

/**
 * Format an ISO-8601 timestamp as a locale date-time, or '—' when absent.
 * Pass a valid IANA `timeZone` string (e.g. `Europe/Moscow`) to override the
 * browser's local timezone. Defaults to the browser timezone when omitted.
 */
export function formatDateTime(
  iso: string | null | undefined,
  timeZone?: string | null,
  language: string | null = currentLocale()
): string {
  if (!iso) return '—';
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(language ?? 'en', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    ...(timeZone ? { timeZone } : {})
  });
}

/** Format an ISO-8601 timestamp as a locale date (no time), or '—' when absent. */
export function formatDate(
  iso: string | null | undefined,
  language: string | null = currentLocale()
): string {
  if (!iso) return '—';
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleDateString(language ?? 'en');
}

/** Compact, human-friendly relative time (e.g. "3m ago", "2d ago"). */
export function formatRelative(
  iso: string | null | undefined,
  language: string | null = currentLocale()
): string {
  if (!iso) return '—';
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const seconds = Math.round((Date.now() - then) / 1000);

  const relative = (key: string, count = 0) => translate(key, { count }, language);
  if (seconds < 5) return relative('relative_now');
  if (seconds < 60) return relative('relative_seconds', seconds);
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return relative('relative_minutes', minutes);
  const hours = Math.round(minutes / 60);
  if (hours < 24) return relative('relative_hours', hours);
  const days = Math.round(hours / 24);
  if (days < 30) return relative('relative_days', days);
  const months = Math.round(days / 30);
  if (months < 12) return relative('relative_months', months);
  return relative('relative_years', Math.round(months / 12));
}

/** Format a (possibly large) event count compactly, e.g. 12.3k. */
export function formatCount(n: number, language: string | null = currentLocale()): string {
  if (language === 'ru')
    return new Intl.NumberFormat('ru', { notation: 'compact', maximumFractionDigits: 1 }).format(n);
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
