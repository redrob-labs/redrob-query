#!/usr/bin/env node
// Copy one platform leg's bundler output into the directory that becomes its `release-*` artifact,
// under the names the release will carry.
//
// The shared publish job (redrob-labs/.github release-publish.yml) uploads whatever the build legs
// put in their artifacts, byte for byte and name for name. So the names are decided HERE, on the
// runner that built the files, instead of being patched on the release afterwards by asset id as
// an earlier version of this pipeline did:
//
//   - installers, the macOS updater payloads and their detached `.sig` files take the
//     launcher-readable name `redrob-query-<version>-<os>-<arch>.<ext>` (launcher-asset-name.mjs);
//   - tauri writes the macOS updater payload as `Redrob Query.app.tar.gz` with neither version nor
//     architecture; it becomes `redrob-query-<version>-macos-<arch>.app.tar.gz`, the same
//     convention as every other asset (and what redrob-recall ships), so the two macOS legs do not
//     overwrite each other in the merged artifact. <arch> comes from the build target
//     (`aarch64-apple-darwin` -> `macos-aarch64`, `x86_64-apple-darwin` -> `macos-x86_64`);
//   - anything else keeps the bundler's name with every space replaced by a dot, which is the
//     name GitHub would store it under anyway -- written out explicitly so latest.json can name
//     the asset before it exists on the release.
//
// Renaming a file does not touch its bytes, so the updater signature made over it stays valid.
//
// Usage: node scripts/stage-release-assets.mjs <bundle-dir> <out-dir> <version> <target>
//   <bundle-dir> is tauri's `target/<triple>/release/bundle`; <version> has no leading "v";
//   <target> is the Rust target triple the leg built.

import { copyFile, mkdir, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { isLauncherInstallable, launcherAssetName, STEM } from './launcher-asset-name.mjs';

// What leaves the bundle directory. Anything else tauri writes (the unpacked .app, the .deb's
// working tree, NSIS temp files) stays behind.
export const RELEASE_FILE_SUFFIXES = ['.AppImage', '.deb', '.dmg', '.msi', '-setup.exe', '.app.tar.gz'];

const BUNDLER_NAME = /^Redrob[. ]Query_(?<version>[^_]+)_/;

// Rust target triple -> the launcher os-arch token the updater payload is published under.
export const PAYLOAD_ARCH = {
  'aarch64-apple-darwin': 'macos-aarch64',
  'x86_64-apple-darwin': 'macos-x86_64',
};

const UNVERSIONED_PAYLOAD = /^Redrob[. ]Query(?<suffix>\.app\.tar\.gz(\.sig)?)$/;

/** True for a release file or the detached signature of one. */
export function isReleaseFile(name) {
  const base = name.endsWith('.sig') ? name.slice(0, -'.sig'.length) : name;
  return RELEASE_FILE_SUFFIXES.some((suffix) => base.endsWith(suffix));
}

/**
 * The name a bundler file is published under.
 * @param {string} name     file name as the bundler wrote it
 * @param {string} version  release version without the leading "v"
 * @param {string} target   Rust target triple of the leg that built it
 */
export function stagedName(name, version, target) {
  const renamed = launcherAssetName(name, version);
  if (renamed !== null) return renamed;
  const payload = UNVERSIONED_PAYLOAD.exec(name);
  if (payload) {
    const arch = PAYLOAD_ARCH[target];
    if (!arch) throw new Error(`${name}: no updater payload architecture is defined for target ${target}`);
    return `${STEM}-${version}-${arch}${payload.groups.suffix}`;
  }
  const match = BUNDLER_NAME.exec(name);
  if (match && match.groups.version !== version) {
    throw new Error(`${name} carries version ${match.groups.version}, not ${version}`);
  }
  return name.replace(/ /g, '.');
}

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      // An unpacked macOS bundle; its .app.tar.gz sibling is the file that ships.
      if (entry.name.endsWith('.app')) continue;
      yield* walk(path);
    } else if (entry.isFile()) {
      yield { name: entry.name, path };
    }
  }
}

/**
 * Stage every release file under `bundleDir` into `outDir`.
 * @returns {Promise<string[]>} the staged names, sorted
 */
export async function stageReleaseAssets(bundleDir, outDir, version, target) {
  const staged = new Map();
  for await (const file of walk(bundleDir)) {
    if (!isReleaseFile(file.name)) continue;
    const published = stagedName(file.name, version, target);
    const previous = staged.get(published);
    if (previous) {
      throw new Error(`${file.path} and ${previous} would both be published as ${published}`);
    }
    staged.set(published, file.path);
  }
  if (staged.size === 0) {
    throw new Error(`no release file under ${bundleDir} (looked for ${RELEASE_FILE_SUFFIXES.join(' ')})`);
  }

  // Every installer must land on a name Redrob Launcher reads. Judged on the final names, so a
  // bundler rename upstream, or a new leg whose arch is not in the table, fails here instead of
  // shipping a release the launcher cannot see.
  const unreadable = [...staged.keys()].filter(
    (name) => /\.(AppImage|deb|dmg|msi)$/.test(name) && !isLauncherInstallable(name),
  );
  if (unreadable.length > 0) {
    throw new Error(`installers Redrob Launcher cannot read:\n- ${unreadable.join('\n- ')}`);
  }

  await mkdir(outDir, { recursive: true });
  for (const [target, source] of staged) await copyFile(source, join(outDir, target));
  return [...staged.keys()].sort();
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [, , bundleDir, outDir, version, target] = process.argv;
  if (!bundleDir || !outDir || !version || !target) {
    console.error('usage: stage-release-assets.mjs <bundle-dir> <out-dir> <version> <target>');
    process.exit(2);
  }
  try {
    const names = await stageReleaseAssets(bundleDir, outDir, version, target);
    console.log(`Staged ${names.length} file(s) into ${outDir}:`);
    for (const name of names) console.log(`  ${name}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
