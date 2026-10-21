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
