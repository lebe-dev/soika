// Maps a Sentry-style severity level (`fatal`, `error`, `warning`, `info`,
// `debug`) to a cohesive set of Tailwind classes used to drive the issue page's
// "severity-driven" accent: a side rail, a filled level pill, a soft hero tint
// and a left accent border. Colours are semantic built-in Tailwind palettes
// (not the brand `primary`), with explicit light/dark variants. Class strings
// are written out in full so Tailwind's scanner picks them up.
import OctagonAlert from '@lucide/svelte/icons/octagon-alert';
import CircleX from '@lucide/svelte/icons/circle-x';
import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
import Info from '@lucide/svelte/icons/info';
import Bug from '@lucide/svelte/icons/bug';

export type SeverityStyle = {
  /** Uppercase label shown in the level pill. */
  label: string;
  /** Solid fill for the vertical side rail. */
  rail: string;
  /** Filled, colour-coded level badge (bg + text + border). */
  pill: string;
  /** Soft background tint for the hero block. */
  tint: string;
  /** Left accent border for cards/frames tied to the error. */
  accentBorder: string;
  /** Lucide icon component for the level. */
  icon: typeof OctagonAlert;
};

const STYLES: Record<string, SeverityStyle> = {
  fatal: {
    label: 'Fatal',
    rail: 'bg-red-500',
    pill: 'bg-red-500/10 text-red-700 dark:text-red-400 border-red-500/30',
    tint: 'bg-red-500/[0.04]',
    accentBorder: 'border-red-500/40',
    icon: OctagonAlert
  },
  error: {
    label: 'Error',
    rail: 'bg-orange-500',
    pill: 'bg-orange-500/10 text-orange-700 dark:text-orange-400 border-orange-500/30',
    tint: 'bg-orange-500/[0.04]',
    accentBorder: 'border-orange-500/40',
    icon: CircleX
  },
  warning: {
    label: 'Warning',
    rail: 'bg-amber-500',
    pill: 'bg-amber-500/10 text-amber-700 dark:text-amber-400 border-amber-500/30',
    tint: 'bg-amber-500/[0.04]',
    accentBorder: 'border-amber-500/40',
    icon: TriangleAlert
  },
  info: {
    label: 'Info',
    rail: 'bg-sky-500',
    pill: 'bg-sky-500/10 text-sky-700 dark:text-sky-400 border-sky-500/30',
    tint: 'bg-sky-500/[0.04]',
    accentBorder: 'border-sky-500/40',
    icon: Info
  },
  debug: {
    label: 'Debug',
    rail: 'bg-slate-400',
    pill: 'bg-slate-500/10 text-slate-600 dark:text-slate-300 border-slate-500/30',
    tint: 'bg-slate-500/[0.03]',
    accentBorder: 'border-slate-500/40',
    icon: Bug
  }
};

const FALLBACK: SeverityStyle = {
  label: 'Unknown',
  rail: 'bg-slate-400',
  pill: 'bg-slate-500/10 text-slate-600 dark:text-slate-300 border-slate-500/30',
  tint: 'bg-slate-500/[0.03]',
  accentBorder: 'border-slate-500/40',
  icon: Bug
};

/** Resolve the accent style for a (possibly null/free-form) severity level. */
export function severityStyle(level: string | null | undefined): SeverityStyle {
  if (!level) return FALLBACK;
  const key = level.toLowerCase().trim();
  // Sentry also emits `critical`; treat it as fatal.
  if (key === 'critical') return STYLES.fatal;
  return STYLES[key] ?? { ...FALLBACK, label: level };
}
