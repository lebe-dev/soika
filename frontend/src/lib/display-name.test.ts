// Tests for the display-name rule mirrored from the backend: at least
// MIN_DISPLAY_NAME_LENGTH characters after trimming, counted as characters
// (code points), not UTF-16 units or bytes.

import { describe, expect, it } from 'vitest';
import { MIN_DISPLAY_NAME_LENGTH, displayNameError } from './display-name';

describe('displayNameError', () => {
  it('rejects a single dot', () => {
    expect(displayNameError('.')).toBe(
      `Display name must be at least ${MIN_DISPLAY_NAME_LENGTH} characters`
    );
  });

  it('rejects blank and too-short names after trimming', () => {
    expect(displayNameError('   ')).not.toBeNull();
    expect(displayNameError('  ab  ')).not.toBeNull();
  });

  it('accepts a name at the minimum length', () => {
    expect(displayNameError('Bob')).toBeNull();
    expect(displayNameError('  Bob  ')).toBeNull();
  });

  it('counts characters, not UTF-16 code units', () => {
    // Two emoji are four UTF-16 units but only two characters.
    expect(displayNameError('🙂🙂')).not.toBeNull();
    expect(displayNameError('Яна')).toBeNull();
  });
});
