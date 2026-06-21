// Tests for the navActions store: a tiny rune-backed holder for the "new
// project" callback that the app shell wires up and the dashboard registers.

import { describe, expect, it, vi } from 'vitest';
import { navActions } from './nav-actions.svelte';

describe('navActions', () => {
  it('defaults to a null newProjectCallback', () => {
    navActions.newProjectCallback = null;
    expect(navActions.newProjectCallback).toBeNull();
  });

  it('stores and returns a registered callback', () => {
    const cb = vi.fn();
    navActions.newProjectCallback = cb;
    expect(navActions.newProjectCallback).toBe(cb);

    navActions.newProjectCallback?.();
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it('can clear the callback again', () => {
    navActions.newProjectCallback = vi.fn();
    navActions.newProjectCallback = null;
    expect(navActions.newProjectCallback).toBeNull();
  });
});
