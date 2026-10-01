// SPDX-License-Identifier: GPL-3.0-or-later
//
// Copy installed plugin packages' prebuilt files into public/, where the app serves them.
//
// Generated rather than committed. The ER diagram's dist is 1.8 MB of third-party build output, and
// committing build output means every version bump lands as a megabyte-scale binary diff in this
// repository's history. The npm package is the pinned artefact -- docs/upstream-sources.toml records
// the version and the licence -- so this script only places what that pin already decided.
//
// Runs before dev, build and preview, because all three serve public/ and a missing plugin there is
// exactly the failure the registry's `installed` flag is there to prevent.

import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

/**
 * Plugins whose files this app serves.
 *
 * `id` must match the registry entry in src/plugins/registry.ts, since that is what maps a view key
 * to the base URL these files land on.
 */
const PLUGINS = [
  {
    id: 'bks-er-diagram',
    package: '@beekeeperstudio/bks-er-diagram',
    // Copied from the package root, so the manifest travels with its own dist.
    include: ['dist', 'manifest.json', 'LICENSE'],
  },
];

/**
 * Source maps are excluded, and the reason is arithmetic rather than taste.
 *
 * The ER diagram ships a 3.9 MB .js.map beside a 1.3 MB .js -- three quarters of the package by
 * size. It is debug data for third-party code we do not debug, and including it would put 5.6 MB into
 * every build instead of 1.8 MB. If someone ever does need to debug inside a plugin, the map is one
 * `npm pack` away.
 */
const EXCLUDE = /\.map$/;

const pluginRoot = (packageName) => {
  // Resolve through the package's own manifest rather than guessing node_modules/<name>: that is what
  // makes this work under a workspace, a pnpm store or a hoisted install.
  const manifestPath = require.resolve(`${packageName}/package.json`);
  return dirname(manifestPath);
};

/**
 * Injected into every plugin's entry HTML, before its own scripts run.
 *
 * The sandbox that makes plugin content safe (`allow-scripts` WITHOUT `allow-same-origin`) gives the
 * frame an opaque origin, and reading `window.localStorage` from an opaque origin throws
 * SecurityError rather than returning null. That is fatal at startup, not merely degraded: the ER
 * diagram reads localStorage in a ref initialiser (`show-all-columns`) and in a store's state factory
 * (`debug-ui`), both of which run while the app is being created, so the throw would leave a blank
 * frame with no error the user can see.
 *
 * Measured in the installed bundle: 4 localStorage references, 0 sessionStorage, 0 document.cookie.
 *
 * The shim is in-memory and per-mount, so a plugin's own UI preferences do not survive reopening the
 * view. That is the honest cost, and it is the right way round: the protocol already carries
 * getViewState/setViewState for state a plugin wants persisted, and the alternative -- granting
 * allow-same-origin -- would put untrusted third-party code in the application's own origin to save a
 * checkbox's position.
 *
 * It installs itself ONLY if the real storage is unreachable, so a future arrangement that serves
 * plugins from their own distinct origin gets genuine persistent storage and this steps aside.
 */
const STORAGE_SHIM = `<script>(function(){
  try { window.localStorage.getItem("probe"); return; } catch (e) {}
  var make = function () {
    var map = new Map();
    return {
      get length() { return map.size; },
      key: function (i) { return Array.from(map.keys())[i] ?? null; },
      getItem: function (k) { return map.has(String(k)) ? map.get(String(k)) : null; },
      setItem: function (k, v) { map.set(String(k), String(v)); },
      removeItem: function (k) { map.delete(String(k)); },
      clear: function () { map.clear(); }
    };
  };
  for (var name of ["localStorage", "sessionStorage"]) {
    try { Object.defineProperty(window, name, { value: make(), configurable: true }); } catch (e) {}
  }
})();</script>`;

let copied = 0;
let skipped = 0;
const report = [];

for (const plugin of PLUGINS) {
  let source;
  try {
    source = pluginRoot(plugin.package);
  } catch {
    // A missing package leaves `installed` false in the registry and the app shows its empty state,
    // so this is a warning rather than a failed build -- an optional view must not stop the app from
    // being built.
    report.push(`  ${plugin.id}: package ${plugin.package} is not installed, skipping`);
    skipped += 1;
    continue;
  }

  const destination = join(ROOT, 'public', 'plugins', plugin.id);
  rmSync(destination, { recursive: true, force: true });
  mkdirSync(destination, { recursive: true });

  for (const entry of plugin.include) {
    const from = join(source, entry);
    if (!existsSync(from)) {
      report.push(`  ${plugin.id}: ${entry} is not in the package, skipping`);
      continue;
    }
    cpSync(from, join(destination, entry), {
      recursive: true,
      filter: (path) => !EXCLUDE.test(path),
    });
  }

  // Two edits to the shipped entry HTML, both recorded because editing third-party content should
  // never be silent.
  const indexPath = join(destination, 'dist', 'index.html');
  if (existsSync(indexPath)) {
    const before = readFileSync(indexPath, 'utf8');
    // 1. The plugin references /vite.svg absolutely, which resolves against the APP root rather than
    //    the plugin's directory -- the app's own favicon would answer a request the plugin made for
    //    its own.
    let after = before.replace(/(href|src)="\/(?!\/)/g, '$1="./');
    if (after !== before) {
      report.push(`  ${plugin.id}: rewrote absolute asset paths in dist/index.html to relative`);
    }
    // 2. The storage shim, inserted at the top of <head> so it runs before the plugin's own module
    //    script, which is deferred by virtue of being type="module".
    if (!after.includes('redrob-plugin-storage-shim')) {
      const shim = STORAGE_SHIM.replace('<script>', '<script data-redrob-plugin-storage-shim>');
      after = after.replace(/<head>/i, `<head>\n    ${shim}`);
      report.push(`  ${plugin.id}: injected the opaque-origin storage shim`);
    }
    if (after !== before) writeFileSync(indexPath, after);
  }

  const version = JSON.parse(readFileSync(join(source, 'package.json'), 'utf8')).version;
  report.push(`  ${plugin.id}: ${plugin.package} ${version} -> ${relative(ROOT, destination)}`);
  copied += 1;
}

console.log(`plugin assets: ${copied} installed, ${skipped} skipped`);
for (const line of report) console.log(line);
