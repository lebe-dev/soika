import { describe, expect, it } from 'vitest';
import deepmerge from 'deepmerge';

describe('vendored deepmerge prototype protection', () => {
  it('blocks malicious keys from both source and target, including null-prototype objects', () => {
    const poisoned = JSON.parse(
      '{"__proto__":{"isAdmin":true},"constructor":{"prototype":{"isAdmin":true}},"prototype":{"isAdmin":true}}'
    );
    for (const target of [{}, Object.create(null), poisoned]) {
      const merged = deepmerge<Record<string, unknown>>(target, poisoned);
      expect(Object.getPrototypeOf(merged)).toBe(Object.prototype);
      expect((merged as Record<string, unknown>).isAdmin).toBeUndefined();
      for (const key of ['__proto__', 'constructor', 'prototype']) {
        expect(Object.hasOwn(merged, key)).toBe(false);
      }
    }
    expect(({} as Record<string, unknown>).isAdmin).toBeUndefined();
  });

  it('retains nested merge, array options and symbol properties used by build tools', () => {
    const token = Symbol('config');
    const merged = deepmerge(
      { nested: { a: 1 }, list: [1] },
      { nested: { b: 2 }, list: [2], [token]: 'ok' },
      { arrayMerge: (_target, source) => source }
    );
    expect(merged).toEqual({ nested: { a: 1, b: 2 }, list: [2], [token]: 'ok' });
  });
});
