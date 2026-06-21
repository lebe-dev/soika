// Tests for the `cn` class-merge helper: it concatenates class lists and lets
// Tailwind-merge resolve conflicting utilities (last one wins).

import { describe, expect, it } from 'vitest';
import { cn } from './utils';

describe('cn', () => {
  it('joins multiple class strings', () => {
    expect(cn('a', 'b', 'c')).toBe('a b c');
  });

  it('drops falsy / conditional values', () => {
    expect(cn('a', false && 'b', null, undefined, 'c')).toBe('a c');
  });

  it('lets conflicting Tailwind utilities resolve to the last one', () => {
    expect(cn('p-2', 'p-4')).toBe('p-4');
  });
});
