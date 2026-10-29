import { afterEach, describe, expect, it, vi } from 'vitest';
import { readJson, writeJson } from './local-storage';

afterEach(() => vi.unstubAllGlobals());

describe('persisted UI preferences', () => {
  it('round-trips JSON values and distinguishes missing preferences', () => {
    const values = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value)
    });
    expect(readJson('missing')).toBeNull();
    writeJson('filters', { levels: ['error'], unread: false });
    expect(readJson('filters')).toEqual({ levels: ['error'], unread: false });
    values.set('filters', '{broken');
    expect(readJson('filters')).toBeNull();
  });

  it('keeps the UI usable when browser storage is unavailable or full', () => {
    vi.stubGlobal('localStorage', {
      getItem() {
        throw new Error('Storage blocked');
      },
      setItem() {
        throw new Error('Quota exceeded');
      }
    });
    expect(readJson('filters')).toBeNull();
    expect(() => writeJson('filters', ['error'])).not.toThrow();
    vi.stubGlobal('localStorage', undefined);
    expect(readJson('filters')).toBeNull();
    expect(() => writeJson('filters', ['error'])).not.toThrow();
  });
});
