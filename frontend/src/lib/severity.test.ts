import { describe, expect, it } from 'vitest';
import { severityStyle } from './severity';

describe('severityStyle', () => {
  it('returns the fallback (label "Unknown") for nullish input', () => {
    const fromNull = severityStyle(null);
    const fromUndefined = severityStyle(undefined);
    const fromEmpty = severityStyle('');

    expect(fromNull.label).toBe('Unknown');
    expect(fromUndefined.label).toBe('Unknown');
    expect(fromEmpty.label).toBe('Unknown');
    // Fallback uses the slate palette + Bug icon.
    expect(fromNull.rail).toBe('bg-slate-400');
    expect(fromNull.accentBorder).toBe('border-slate-500/40');
  });

  it('resolves each known level by exact key', () => {
    expect(severityStyle('fatal').label).toBe('Fatal');
    expect(severityStyle('error').label).toBe('Error');
    expect(severityStyle('warning').label).toBe('Warning');
    expect(severityStyle('info').label).toBe('Info');
    expect(severityStyle('debug').label).toBe('Debug');
  });

  it('treats "critical" as an alias for fatal (case-insensitive + trimmed)', () => {
    const fatal = severityStyle('fatal');
    expect(severityStyle('CRITICAL')).toEqual(fatal);
    expect(severityStyle(' fatal ')).toEqual(fatal);
    // Sanity: it is the red/fatal style object, not the fallback.
    expect(fatal.label).toBe('Fatal');
    expect(fatal.rail).toBe('bg-red-500');
  });

  it('is case-insensitive and trims surrounding whitespace for known levels', () => {
    const warning = severityStyle('warning');
    expect(severityStyle('  WARNING  ')).toEqual(warning);
    expect(severityStyle('Warning')).toEqual(warning);
  });

  it('returns the amber/warning style for "warning"', () => {
    const warning = severityStyle('warning');
    expect(warning.label).toBe('Warning');
    expect(warning.rail).toBe('bg-amber-500');
    expect(warning.pill).toBe(
      'bg-amber-500/10 text-amber-700 dark:text-amber-400 border-amber-500/30'
    );
    expect(warning.tint).toBe('bg-amber-500/[0.04]');
    expect(warning.accentBorder).toBe('border-amber-500/40');
  });

  it('returns the fallback style for an unknown level but preserves the original label', () => {
    const notice = severityStyle('notice');
    // Falls back to the slate palette / Bug icon...
    expect(notice.rail).toBe('bg-slate-400');
    expect(notice.pill).toBe(
      'bg-slate-500/10 text-slate-600 dark:text-slate-300 border-slate-500/30'
    );
    expect(notice.accentBorder).toBe('border-slate-500/40');
    // ...but the label keeps the original (non-"Unknown") casing.
    expect(notice.label).toBe('notice');
  });
});
