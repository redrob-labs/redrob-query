// SPDX-License-Identifier: GPL-3.0-or-later
//
// The storage shim, tested against the artifact that actually ships.
//
// It reads the generated public/plugins/.../index.html rather than a copy of the source string, so a
// change to the install script that breaks the injection is caught here. `pretest` runs
// plugins:install, so the file is present.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const ENTRY = join(process.cwd(), 'public/plugins/bks-er-diagram/dist/index.html');

const shimSource = (): string => {
  const html = readFileSync(ENTRY, 'utf8');
  const match = html.match(/<script data-redrob-plugin-storage-shim>([\s\S]*?)<\/script>/);
  if (!match) throw new Error('the storage shim is not in the installed entry HTML');
  return match[1];
};

/** Run the shim against a stand-in window, as an opaque-origin frame would. */
const runShim = (storageThrows: boolean) => {
  const fake: Record<string, unknown> = {};
  // A reachable storage must actually WORK, not merely be present. A first version of this fixture
  // returned an object whose getItem threw in both cases, so the shim installed itself either way and
  // the "steps aside" assertion failed -- the fixture was wrong, not the shim.
  const working: Storage = {
    length: 0,
    clear: () => {},
    getItem: () => null,
    key: () => null,
    removeItem: () => {},
    setItem: () => {},
  };
  Object.defineProperty(fake, 'localStorage', {
    get: () => {
      if (storageThrows) throw new Error('SecurityError: access is denied for this document');
      return working;
    },
    configurable: true,
  });
  Object.defineProperty(fake, 'sessionStorage', { value: working, configurable: true });

  // eslint-disable-next-line no-new-func
  new Function('window', shimSource())(fake);
  return fake;
};

describe('plugin storage shim', () => {
  it('is present in the installed entry HTML', () => {
    expect(existsSync(ENTRY), `${ENTRY} should exist; pretest runs plugins:install`).toBe(true);
    expect(shimSource()).toContain('localStorage');
  });

  it('runs before the plugin module, which is deferred', () => {
    const html = readFileSync(ENTRY, 'utf8');
    const shimAt = html.indexOf('data-redrob-plugin-storage-shim');
    const moduleAt = html.indexOf('type="module"');
    expect(shimAt).toBeGreaterThan(-1);
    expect(moduleAt).toBeGreaterThan(-1);
    // A classic inline script runs before any type="module" script regardless of order, but placing
    // it first keeps that from depending on a subtlety.
    expect(shimAt).toBeLessThan(moduleAt);
  });

  it('installs a working Storage when the real one throws', () => {
    // The case the sandbox creates. The plugin reads localStorage in a ref initialiser and a store
    // state factory, so a throw here is a blank diagram rather than a lost preference.
    const fake = runShim(true);
    const storage = fake.localStorage as Storage;

    expect(storage.getItem('show-all-columns')).toBeNull();
    storage.setItem('show-all-columns', 'true');
    expect(storage.getItem('show-all-columns')).toBe('true');
    expect(storage.length).toBe(1);
    expect(storage.key(0)).toBe('show-all-columns');

    storage.setItem('debug-ui', 'false');
    expect(storage.length).toBe(2);
    storage.removeItem('show-all-columns');
    expect(storage.getItem('show-all-columns')).toBeNull();
    storage.clear();
    expect(storage.length).toBe(0);
    expect(storage.key(0)).toBeNull();
  });

  it('coerces keys and values to strings, as Storage does', () => {
    const storage = runShim(true).localStorage as Storage;
    // The plugin writes booleans via String(), but a plugin that passes a number must not get one
    // back -- real Storage always returns a string.
    (storage as unknown as { setItem(k: unknown, v: unknown): void }).setItem(1, 2);
    expect(storage.getItem('1')).toBe('2');
    expect(typeof storage.getItem('1')).toBe('string');
  });

  it('shims sessionStorage as well', () => {
    const storage = runShim(true).sessionStorage as Storage;
    storage.setItem('a', 'b');
    expect(storage.getItem('a')).toBe('b');
  });

  it('steps aside when the real storage is reachable', () => {
    // So that serving plugins from their own distinct origin later gives them genuine persistence
    // without this having to be removed first.
    const fake = runShim(false);
    const storage = fake.localStorage as Storage;
    // The shim's own object has a `length` getter over a Map; the untouched fixture has a plain 0 and
    // a getItem that always returns null. Writing then reading tells them apart.
    storage.setItem('a', 'b');
    expect(storage.getItem('a')).toBeNull();
  });
});
