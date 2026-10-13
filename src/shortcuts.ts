// Every app-wide shortcut, in one table (Beekeeper keeps its keymap as data, not in handlers). The
// handlers match against it and every hint on screen is written from it, so a hint cannot name a key
// the handler does not use -- and on Linux and Windows the hint says Ctrl, not the Mac's ⌘.

export interface Shortcut {
  /** The key as KeyboardEvent.key reports it, lower case for letters. */
  key: string;
  /** Cmd on macOS, Ctrl elsewhere. */
  mod?: boolean;
  shift?: boolean;
}

export const SHORTCUTS = {
  palette: { key: 'k', mod: true },
  runQuery: { key: 'Enter', mod: true },
  saveQuery: { key: 's', mod: true },
  newQuery: { key: 'n', mod: true },
  askAi: { key: 'i', mod: true },
  editCell: { key: 'Enter', shift: true },
} as const satisfies Record<string, Shortcut>;

export type ShortcutId = keyof typeof SHORTCUTS;

export const isMac = (platform: string = typeof navigator === 'undefined' ? '' : navigator.platform): boolean => /mac|iphone|ipad/i.test(platform);

export function matches(event: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey'>, id: ShortcutId, mac = isMac()): boolean {
  const shortcut: Shortcut = SHORTCUTS[id];
  const mod = mac ? event.metaKey : event.ctrlKey;
  return event.key.toLowerCase() === shortcut.key.toLowerCase() && mod === Boolean(shortcut.mod) && event.shiftKey === Boolean(shortcut.shift);
}

const KEY_NAMES: Record<string, string> = { enter: '↵' };

/** "⌘ K" on a Mac, "Ctrl K" elsewhere. */
export function formatShortcut(id: ShortcutId, mac = isMac()): string {
  const shortcut: Shortcut = SHORTCUTS[id];
  const key = KEY_NAMES[shortcut.key.toLowerCase()] ?? shortcut.key.toUpperCase();
  return [shortcut.mod ? (mac ? '⌘' : 'Ctrl') : null, shortcut.shift ? (mac ? '⇧' : 'Shift') : null, key].filter(Boolean).join(' ');
}
