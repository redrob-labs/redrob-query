import { describe, expect, it } from 'vitest';
import { formatShortcut, isMac, matches } from './shortcuts';

const key = (k: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }> = {}) => ({ key: k, metaKey: false, ctrlKey: false, shiftKey: false, ...mods });

describe('shortcuts', () => {
  it('writes Ctrl off a Mac and ⌘ on one', () => {
    expect(formatShortcut('palette', false)).toBe('Ctrl K');
    expect(formatShortcut('palette', true)).toBe('⌘ K');
    expect(formatShortcut('runQuery', false)).toBe('Ctrl ↵');
    expect(formatShortcut('editCell', false)).toBe('Shift ↵');
  });

  it('matches the platform modifier only', () => {
    expect(matches(key('k', { ctrlKey: true }), 'palette', false)).toBe(true);
    expect(matches(key('k', { metaKey: true }), 'palette', false)).toBe(false);
    expect(matches(key('K', { metaKey: true }), 'palette', true)).toBe(true);
  });

  it('does not match with an extra or a missing modifier', () => {
    expect(matches(key('Enter', { ctrlKey: true, shiftKey: true }), 'runQuery', false)).toBe(false);
    expect(matches(key('Enter'), 'editCell', false)).toBe(false);
    expect(matches(key('Enter', { shiftKey: true }), 'editCell', false)).toBe(true);
  });

  it('reads the platform', () => {
    expect(isMac('MacIntel')).toBe(true);
    expect(isMac('Linux x86_64')).toBe(false);
    expect(isMac('Win32')).toBe(false);
  });
});
