// SPDX-License-Identifier: GPL-3.0-or-later
//
// Which plugins this build knows about, and where their files are served from.
//
// Deliberately a static registry rather than a directory scan. Discovering plugins from disk means
// the set of code the app will load is decided at runtime by whatever is in a folder, and in a Tauri
// app that folder is user-writable. Installing a plugin should be an explicit act with a version and
// a licence recorded in docs/upstream-sources.toml, so it is a build-time list here and the loader
// only ever mounts something this file names.

import { PluginManifestError, readManifest, type PluginManifest, type PluginViewSpec } from './manifest';

/** A plugin this build can mount, with its manifest already validated. */
export interface RegisteredPlugin {
  manifest: PluginManifest;
  /** Base URL the plugin's own files are served from, with a trailing slash. */
  baseUrl: string;
  /**
   * Whether this plugin's FILES are actually present in the build.
   *
   * Separate from being registered, and the distinction was found by checking rather than assumed.
   * A registered plugin whose files are missing does not produce a 404 in this app: the SPA fallback
   * answers any unmatched path with index.html, so the iframe loads a nested copy of Redrob Query
   * instead of the plugin. Measured against a `vite preview` build -- the response for
   * /plugins/bks-er-diagram/dist/index.html was byte-identical to the app's own index.
   *
   * The sandbox keeps that from being a security problem, but it is still the wrong picture shown
   * confidently, which is worse than an honest empty state. So a plugin is mounted only when its
   * files are known to be here.
   */
  installed: boolean;
}

/**
 * The ER diagram's manifest, as published in @beekeeperstudio/bks-er-diagram 1.1.2.
 *
 * Mirrored here rather than imported, because the package's own manifest.json is not part of its
 * module graph -- it ships alongside a prebuilt dist/index.html and is read by the host, not by a
 * bundler. Keeping it here means the shape is validated by readManifest at startup like any other,
 * and the pinned version in docs/upstream-sources.toml is the thing that must match.
 */
const erDiagramManifest = {
  id: 'bks-er-diagram',
  name: 'ER Diagram',
  description: 'Interactive ER Diagram viewer for relational database schemas',
  version: '1.1.2',
  icon: 'account_tree',
  manifestVersion: 1,
  minAppVersion: '5.5.0',
  capabilities: {
    views: [{ id: 'main-view', name: 'ER Diagram', type: 'plain-tab', entry: 'dist/index.html' }],
    menu: [
      { command: 'showAllEntities', name: 'Open ER Diagram', view: 'main-view', placement: 'menubar.tools' },
      { command: 'showOneSchema', name: 'View ERD', view: 'main-view', placement: 'entity.schema.context' },
      { command: 'showOneTable', name: 'View ERD', view: 'main-view', placement: 'entity.table.context' },
    ],
  },
};

/**
 * `installed` records that the plugin's files are placed under public/plugins/ by
 * scripts/install-plugin-assets.mjs, which runs before dev, build, preview and test.
 *
 * A build-time fact, not a runtime probe, and that is the point: this app answers an unmatched path
 * with its own index.html, so probing the entry URL cannot tell a present plugin from the SPA
 * fallback. The flag and the copy script are edited together -- one names the file, the other puts it
 * there -- so a plugin is only ever offered when something actually installed it.
 */
const REGISTERED = [
  { raw: erDiagramManifest, baseUrl: '/plugins/bks-er-diagram/', installed: true },
];

/**
 * Validate every registered plugin, dropping any that fails.
 *
 * A malformed manifest disables that one plugin and leaves the application usable, which is the
 * right trade for an optional view: refusing to start the whole app because a schema viewer's
 * manifest is wrong would be a worse failure than not offering the viewer. The reason is reported
 * so it is not silent.
 */
export const loadRegisteredPlugins = (
  onError: (pluginDescription: string, reason: string) => void = () => {},
): RegisteredPlugin[] => {
  const loaded: RegisteredPlugin[] = [];
  for (const entry of REGISTERED) {
    try {
      loaded.push({ manifest: readManifest(entry.raw), baseUrl: entry.baseUrl, installed: entry.installed });
    } catch (error) {
      const reason = error instanceof PluginManifestError ? error.message : String(error);
      onError(entry.baseUrl, reason);
    }
  }
  return loaded;
};

/** Resolve a view's entry against its plugin's base URL. */
export const resolvePluginEntry = (plugins: RegisteredPlugin[]) => (
  manifest: PluginManifest,
  view: PluginViewSpec,
): string => {
  const plugin = plugins.find((candidate) => candidate.manifest.id === manifest.id);
  if (!plugin) throw new Error(`plugin not registered: ${manifest.id}`);
  return `${plugin.baseUrl}${view.entry}`;
};

/** Find a registered view by the `<pluginId>/<viewId>` key the store holds. */
export const findPluginView = (
  plugins: RegisteredPlugin[],
  key: string,
): { plugin: RegisteredPlugin; view: PluginViewSpec } | null => {
  const separator = key.indexOf('/');
  if (separator <= 0) return null;
  const pluginId = key.slice(0, separator);
  const viewId = key.slice(separator + 1);
  const plugin = plugins.find((candidate) => candidate.manifest.id === pluginId);
  if (!plugin) return null;
  const view = plugin.manifest.views.find((candidate) => candidate.id === viewId);
  return view ? { plugin, view } : null;
};

/** The key the store holds for one view. */
export const pluginViewKey = (pluginId: string, viewId: string): string => `${pluginId}/${viewId}`;
