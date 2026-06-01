import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { formatCount, formatDate, formatDateTime, formatRelative } from './format';

describe('formatCount', () => {
  it('renders raw numbers below 1000', () => {
    expect(formatCount(0)).toBe('0');
    expect(formatCount(42)).toBe('42');
    expect(formatCount(999)).toBe('999');
  });

  it('renders thousands with one decimal below 10k, none above', () => {
    expect(formatCount(1000)).toBe('1.0k');
    expect(formatCount(1500)).toBe('1.5k');
    expect(formatCount(12300)).toBe('12k');
    expect(formatCount(999999)).toBe('1000k');
  });

  it('renders millions with one decimal', () => {
    expect(formatCount(1_000_000)).toBe('1.0M');
    expect(formatCount(2_500_000)).toBe('2.5M');
  });
});

describe('formatDateTime / formatDate', () => {
  it('returns an em dash for nullish input', () => {
    expect(formatDateTime(null)).toBe('—');
    expect(formatDateTime(undefined)).toBe('—');
    expect(formatDate(null)).toBe('—');
    expect(formatDate(undefined)).toBe('—');
  });

  it('returns the original string when it is not a valid date', () => {
    expect(formatDateTime('not-a-date')).toBe('not-a-date');
    expect(formatDate('nonsense')).toBe('nonsense');
  });

  it('formats a valid timestamp into a non-empty locale string', () => {
    const iso = '2026-05-31T12:34:56Z';
    expect(formatDateTime(iso)).not.toBe('—');
    expect(formatDateTime(iso)).not.toBe(iso);
    expect(formatDate(iso)).not.toBe('—');
  });
});

describe('formatRelative', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-05-31T12:00:00Z'));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  const ago = (ms: number) => new Date(Date.now() - ms).toISOString();

  it('returns an em dash for nullish input', () => {
    expect(formatRelative(null)).toBe('—');
    expect(formatRelative(undefined)).toBe('—');
  });

  it('returns the original string for an invalid date', () => {
    expect(formatRelative('bogus')).toBe('bogus');
  });

  it('describes recent times as "just now"', () => {
    expect(formatRelative(ago(2_000))).toBe('just now');
  });

  it('scales the unit with the elapsed time', () => {
    expect(formatRelative(ago(30_000))).toBe('30s ago');
    expect(formatRelative(ago(5 * 60_000))).toBe('5m ago');
    expect(formatRelative(ago(3 * 3_600_000))).toBe('3h ago');
    expect(formatRelative(ago(2 * 86_400_000))).toBe('2d ago');
    expect(formatRelative(ago(45 * 86_400_000))).toBe('2mo ago');
    expect(formatRelative(ago(400 * 86_400_000))).toBe('1y ago');
  });
});
