#!/usr/bin/env node
// Write latest.json -- the Tauri updater feed -- from the staged release files, and refuse to
// write one the released clients could not use.
//
// tauri-action used to write this file while uploading to a draft release, and two more steps then
// corrected it (draft asset URLs are REST endpoints the updater cannot fetch; renamed assets had to
// be re-resolved). With the build legs staging their files under their final names (see
// stage-release-assets.mjs) and the shared publish job uploading them unchanged, the published URL
// of every asset is known before the release exists:
//
//     https://github.com/<owner/repo>/releases/download/v<version>/<name>
//
// so the manifest is written once, here, and verified against the bytes beside it:
//
//   - every platform key the build matrix produced is present (REQUIRED_PLATFORMS), and every
//     asset suffix a built bundle type leaves is present (REQUIRED_ASSET_SUFFIXES) -- both come
//     from scripts/release-matrix.mjs, the same single list that built the legs;
//   - every signature verifies against the file it names (verify-updater-signature.mjs), because a
//     signature over different bytes still LOOKS signed and fails on every user's machine at the
//     first update check. The AppImage is repacked after tauri signs it, so this is a live risk.
//
// The platform keys mirror what tauri-action wrote for this app, so an installed client keeps
// resolving the key it already looks up: the plain target key (`linux-x86_64`, `darwin-aarch64`,
// `darwin-x86_64`, `windows-x86_64`) and the per-bundle aliases beside it. Windows prefers the
// NSIS installer, as `updaterJsonPreferNsis` did.
//
// Usage: node scripts/write-updater-manifest.mjs <assets-dir> <version> <owner/repo> <output>
// Env:   TAURI_UPDATER_PUBLIC_KEY (required), REQUIRED_PLATFORMS, REQUIRED_ASSET_SUFFIXES
//        (space-separated; empty means "whatever is there")

import { execFileSync } from 'node:child_process';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Staged name suffix -> the updater platform keys that name it. Order matters only for the
// manifest's readability.
export const UPDATER_KEYS = [
  { suffix: '-linux-x86_64.AppImage', keys: ['linux-x86_64', 'linux-x86_64-appimage'] },
  { suffix: '-linux-x86_64.deb', keys: ['linux-x86_64-deb'] },
  { suffix: '_aarch64.app.tar.gz', keys: ['darwin-aarch64', 'darwin-aarch64-app'] },
  { suffix: '_x64.app.tar.gz', keys: ['darwin-x86_64', 'darwin-x86_64-app'] },
  { suffix: '-windows-x64-setup.exe', keys: ['windows-x86_64', 'windows-x86_64-nsis'] },
  { suffix: '-windows-x64.msi', keys: ['windows-x86_64-msi'] },
];

export function assetUrl(repository, version, name) {
  return `https://github.com/${repository}/releases/download/v${version}/${encodeURIComponent(name)}`;
}

/**
 * Build the manifest from `[{ name, signature }]` -- one entry per signed asset.
 * Throws when a signed asset maps to no platform key, or two assets claim the same key.
 */
export function buildUpdaterManifest({ signed, version, repository, pubDate = new Date() }) {
  const platforms = {};
  for (const { name, signature } of signed) {
    const mapping = UPDATER_KEYS.find((entry) => name.endsWith(entry.suffix));
    if (!mapping) throw new Error(`${name}.sig: no updater platform key is defined for this asset`);
    for (const key of mapping.keys) {
      if (platforms[key]) throw new Error(`${name} and ${platforms[key].name} both claim ${key}`);
      platforms[key] = { name, signature, url: assetUrl(repository, version, name) };
    }
  }
  return {
    version,
    notes: `See https://github.com/${repository}/blob/main/CHANGELOG.md for release notes.`,
    pub_date: pubDate.toISOString(),
    platforms: Object.fromEntries(
      Object.entries(platforms).map(([key, { signature, url }]) => [key, { signature, url }]),
    ),
  };
}

/**
 * Throw unless the manifest covers every required platform, every URL is this repository's own
 * release asset for this version, and `names` carries every required suffix.
 */
export function assertUpdaterContract(manifest, { names, version, repository, requiredPlatforms, requiredSuffixes }) {
  const problems = [];
  if (manifest.version !== version) problems.push(`manifest version ${manifest.version} is not ${version}`);
  const entries = Object.entries(manifest.platforms ?? {});
  if (entries.length === 0) problems.push('manifest lists no platforms at all');
  const prefix = `https://github.com/${repository}/releases/download/v${version}/`;
  for (const [key, entry] of entries) {
    if (!entry.signature) problems.push(`${key} has no signature`);
    if (typeof entry.url !== 'string' || !entry.url.startsWith(prefix)) {
      problems.push(`${key} url is not a release asset of ${repository} v${version}: ${entry.url}`);
    } else if (!names.includes(decodeURIComponent(entry.url.slice(prefix.length)))) {
      problems.push(`${key} names ${entry.url.slice(prefix.length)}, which is not among the staged files`);
    }
  }
  for (const platform of requiredPlatforms) {
    if (!manifest.platforms?.[platform]) problems.push(`missing a signed entry for ${platform}`);
  }
  for (const suffix of requiredSuffixes) {
    if (!names.some((name) => name.endsWith(suffix))) {
      problems.push(`no staged file ends in ${suffix} -- a built platform produced no installer`);
    }
  }
  if (problems.length > 0) throw new Error(`latest.json would not serve the released clients:\n- ${problems.join('\n- ')}`);
}

const splitList = (value) => (value ?? '').split(/\s+/).filter(Boolean);

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [, , assetsDir, version, repository, output] = process.argv;
  if (!assetsDir || !version || !repository || !output) {
    console.error('usage: write-updater-manifest.mjs <assets-dir> <version> <owner/repo> <output>');
    process.exit(2);
  }
  const publicKey = process.env.TAURI_UPDATER_PUBLIC_KEY?.trim();
  if (!publicKey) {
    console.error('TAURI_UPDATER_PUBLIC_KEY is required to verify the updater signatures');
    process.exit(2);
  }
  const verifier = resolve(dirname(fileURLToPath(import.meta.url)), 'verify-updater-signature.mjs');

  try {
    const names = (await readdir(assetsDir)).filter((name) => name !== 'latest.json').sort();
    const signed = [];
    for (const sig of names.filter((name) => name.endsWith('.sig'))) {
      const name = sig.slice(0, -'.sig'.length);
      if (!names.includes(name)) throw new Error(`${sig} has no ${name} beside it`);
      const signature = (await readFile(join(assetsDir, sig), 'utf8')).trim();
      let result;
      try {
        result = execFileSync(process.execPath, [verifier, publicKey, signature, join(assetsDir, name)], {
          encoding: 'utf8',
          stdio: ['ignore', 'pipe', 'inherit'],
        }).trim();
      } catch (error) {
        throw new Error(`${name}: ${error.stdout?.toString().trim() || error.message}`);
      }
      console.log(`  ${name}: ${result}`);
      signed.push({ name, signature });
    }
    if (signed.length === 0) throw new Error(`no .sig file under ${assetsDir}; nothing to publish to the updater`);

    const manifest = buildUpdaterManifest({ signed, version, repository });
    assertUpdaterContract(manifest, {
      names,
      version,
      repository,
      requiredPlatforms: splitList(process.env.REQUIRED_PLATFORMS),
      requiredSuffixes: splitList(process.env.REQUIRED_ASSET_SUFFIXES),
    });
    await writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`);
    const keys = Object.keys(manifest.platforms);
    console.log(`Wrote ${output} for v${version}: ${keys.length} platform keys (${keys.join(' ')}), every signature verified.`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
