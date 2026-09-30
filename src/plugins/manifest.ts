// SPDX-License-Identifier: GPL-3.0-or-later
//
// Reading a plugin's manifest.json.
//
// The shape is measured from a real published plugin (@beekeeperstudio/bks-er-diagram 1.1.2) and the
// public reference at docs/plugin_development/manifest.md in beekeeper-studio at the commit pinned in
// docs/upstream-sources.toml. Nothing is copied.
//
// Every field is validated rather than trusted. A manifest arrives with the plugin, so it is
// third-party input: a missing `entry` would mount an iframe pointing at nothing, and an `entry`
// containing `../` would reach outside the plugin's own directory. Both are rejected here, where the
// error can name the plugin, instead of failing later as a blank tab.

/** A view a plugin contributes. `type` is the host's placement hint. */
export interface PluginViewSpec {
  id: string;
  name: string;
  type: 'plain-tab' | string;
  /** Path to the view's HTML, relative to the plugin's own directory. */
  entry: string;
}

/** A menu item a plugin contributes. `placement` is a host-defined anchor. */
export interface PluginMenuSpec {
  command: string;
  name: string;
  view: string;
  placement: string;
}

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  manifestVersion: number;
  description?: string;
  icon?: string;
  minAppVersion?: string;
  views: PluginViewSpec[];
  menu: PluginMenuSpec[];
}

/** The manifest versions this host understands. Widening this is a deliberate act. */
export const SUPPORTED_MANIFEST_VERSIONS = [1] as const;

export class PluginManifestError extends Error {}

const asRecord = (value: unknown): Record<string, unknown> | null =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const requireString = (source: Record<string, unknown>, key: string, where: string): string => {
  const value = source[key];
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new PluginManifestError(`${where}: ${key} must be a non-empty string`);
  }
  return value;
};

/**
 * Reject an entry path that escapes the plugin's directory or points somewhere else entirely.
 *
 * A manifest is data shipped by a third party, so `entry` is the one field that turns into a URL the
 * host loads. `../` would climb out of the plugin's directory, a leading `/` would resolve against
 * the application root, and a scheme-bearing value would load an arbitrary remote page inside the
 * app. Each is refused by name so the failure says what was wrong.
 */
const requireSafeEntry = (source: Record<string, unknown>, where: string): string => {
  const entry = requireString(source, 'entry', where);
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(entry) || entry.startsWith('//')) {
    throw new PluginManifestError(`${where}: entry must be a relative path, not a URL (${entry})`);
  }
  if (entry.startsWith('/')) {
    throw new PluginManifestError(`${where}: entry must be relative to the plugin, not absolute (${entry})`);
  }
  if (entry.split('/').some((segment) => segment === '..')) {
    throw new PluginManifestError(`${where}: entry must not leave the plugin directory (${entry})`);
  }
  return entry;
};

/** Parse and validate a manifest. `raw` is the already-parsed JSON. */
export const readManifest = (raw: unknown): PluginManifest => {
  const root = asRecord(raw);
  if (!root) throw new PluginManifestError('manifest must be a JSON object');

  const id = requireString(root, 'id', 'manifest');
  const where = `plugin ${id}`;
  const manifestVersion = root.manifestVersion;
  if (typeof manifestVersion !== 'number' || !Number.isInteger(manifestVersion)) {
    throw new PluginManifestError(`${where}: manifestVersion must be an integer`);
  }
  if (!(SUPPORTED_MANIFEST_VERSIONS as readonly number[]).includes(manifestVersion)) {
    // Refused rather than attempted. A future manifest version may move or rename the very fields
    // read below, so guessing produces a plugin that half-works.
    throw new PluginManifestError(
      `${where}: manifestVersion ${manifestVersion} is not supported (this host reads ${SUPPORTED_MANIFEST_VERSIONS.join(', ')})`,
    );
  }

  const capabilities = asRecord(root.capabilities) ?? {};
  const rawViews = Array.isArray(capabilities.views) ? capabilities.views : [];
  const views: PluginViewSpec[] = rawViews.map((candidate, index) => {
    const view = asRecord(candidate);
    if (!view) throw new PluginManifestError(`${where}: view ${index} must be an object`);
    return {
      id: requireString(view, 'id', `${where} view ${index}`),
      name: requireString(view, 'name', `${where} view ${index}`),
      type: typeof view.type === 'string' ? view.type : 'plain-tab',
      entry: requireSafeEntry(view, `${where} view ${index}`),
    };
  });
  if (views.length === 0) {
    throw new PluginManifestError(`${where}: declares no views, so there is nothing to mount`);
  }
  const duplicate = views.find((view, index) => views.findIndex((other) => other.id === view.id) !== index);
  if (duplicate) {
    // View ids key the host's per-view state, so two views sharing one id would silently share it.
    throw new PluginManifestError(`${where}: duplicate view id ${duplicate.id}`);
  }

  const rawMenu = Array.isArray(capabilities.menu) ? capabilities.menu : [];
  const menu: PluginMenuSpec[] = rawMenu.map((candidate, index) => {
    const item = asRecord(candidate);
    if (!item) throw new PluginManifestError(`${where}: menu item ${index} must be an object`);
    const view = requireString(item, 'view', `${where} menu ${index}`);
    if (!views.some((known) => known.id === view)) {
      throw new PluginManifestError(`${where}: menu item ${index} targets unknown view ${view}`);
    }
    return {
      command: requireString(item, 'command', `${where} menu ${index}`),
      name: requireString(item, 'name', `${where} menu ${index}`),
      view,
      placement: requireString(item, 'placement', `${where} menu ${index}`),
    };
  });

  return {
    id,
    name: requireString(root, 'name', where),
    version: requireString(root, 'version', where),
    manifestVersion,
    description: typeof root.description === 'string' ? root.description : undefined,
    icon: typeof root.icon === 'string' ? root.icon : undefined,
    minAppVersion: typeof root.minAppVersion === 'string' ? root.minAppVersion : undefined,
    views,
    menu,
  };
};
