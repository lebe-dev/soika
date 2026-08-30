import { describe, expect, it } from 'vitest';
import { isCeremonyCancelled } from './passkey';

describe('isCeremonyCancelled', () => {
  it('treats a dismissed browser dialog as a cancellation', () => {
    const err = new Error('The operation either timed out or was not allowed.');
    err.name = 'NotAllowedError';
    expect(isCeremonyCancelled(err)).toBe(true);
  });

  it('treats an aborted ceremony as a cancellation', () => {
    const err = new Error('aborted');
    err.name = 'AbortError';
    expect(isCeremonyCancelled(err)).toBe(true);
  });

  it('does not swallow real failures', () => {
    expect(isCeremonyCancelled(new Error('boom'))).toBe(false);
    expect(isCeremonyCancelled('not an error')).toBe(false);
  });
});
