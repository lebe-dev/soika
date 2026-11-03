// Tests for the avatar helpers shared by every roster/identity chip: initials
// are derived from the first and last word, and the tint is a deterministic
// pick from a fixed palette so the same id always renders the same colour.

import { describe, expect, it } from 'vitest';
import { AVATAR_TINTS, avatarTint, initials } from './avatar';

describe('initials', () => {
  it('takes the first letter of the first and last word', () => {
    expect(initials('Anna Maria Smith')).toBe('AS');
  });

  it('uses the first two letters of a single word', () => {
    expect(initials('backend')).toBe('BA');
  });

  it('ignores surrounding and repeated whitespace', () => {
    expect(initials('  dev   team ')).toBe('DT');
  });

  it('falls back to a placeholder for an empty name', () => {
    expect(initials('   ')).toBe('?');
  });

  it('skips special characters instead of putting them in the initials', () => {
    expect(initials('Lab Board (dev)')).toBe('LD');
    expect(initials('(dev) Lab')).toBe('DL');
    expect(initials('lab-board-dev')).toBe('LD');
    expect(initials('(dev)')).toBe('DE');
    expect(initials('@#$ %^&')).toBe('?');
  });

  it('keeps digits and non-Latin letters', () => {
    expect(initials('Проект 42')).toBe('П4');
  });
});

describe('avatarTint', () => {
  it('is deterministic for the same id', () => {
    expect(avatarTint('a1b2c3')).toBe(avatarTint('a1b2c3'));
  });

  it('always returns one of the palette entries', () => {
    for (const id of ['', 'x', 'project-1', 'eaf33044-923a-4cc0-97f5-5622748b3051']) {
      expect(AVATAR_TINTS).toContain(avatarTint(id));
    }
  });

  it('spreads different ids across the palette', () => {
    const ids = Array.from({ length: 50 }, (_, i) => `id-${i}`);
    const used = new Set(ids.map(avatarTint));
    expect(used.size).toBeGreaterThan(1);
  });
});

describe('Unicode identities', () => {
  it('keeps complete code points in initials', () => {
    expect(initials('𝐀lpha 𝐁eta')).toBe('𝐀𝐁');
    expect(initials('𝐀𝐁𝐂')).toBe('𝐀𝐁');
  });

  it('drops emoji from initials', () => {
    expect(initials('😀 Team')).toBe('TE');
    expect(initials('😀')).toBe('?');
  });

  it('hashes supplementary characters as one code point', () => {
    expect(avatarTint('😀')).toBe(AVATAR_TINTS[0x1f600 % AVATAR_TINTS.length]);
  });
});
