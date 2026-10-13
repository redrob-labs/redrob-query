#!/usr/bin/env node
// The release manifest Redrob Launcher reads from every GitHub Release of this app.
//
// One asset named `redrob-launcher.json`, uploaded next to the installers, tells the launcher
// what this release can do without any change on the launcher's side. Schema 1 carries a single
// fact: `sharedAccount` -- whether the release reads the launcher's shared account file, so that
// signing in to the launcher is all a user needs. It is `true` here because develop carries that
// change (the fallback to the launcher's shared account).
//
// The contract this mirrors lives in the launcher repository at docs/release-manifest.md. The
// content below is the exact bytes that document shows; the launcher ignores unknown fields and
// unknown schemas, so growing it later is safe, shrinking it is not.
//
// Usage: node scripts/launcher-release-manifest.mjs <output-path>
//   writes the manifest to <output-path> (parent directories are created).

import { mkdir, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const LAUNCHER_MANIFEST_NAME = 'redrob-launcher.json';

export const LAUNCHER_MANIFEST_SCHEMA = 1;

// Set to false only if a release stops reading the shared account file; the launcher then
// treats the app as needing its own sign-in, whatever its built-in table says.
export const SHARED_ACCOUNT = true;

/** The newline-terminated bytes of the manifest, exactly as the contract writes them. */
export function launcherManifestContent() {
  return `{ "schema": ${LAUNCHER_MANIFEST_SCHEMA}, "sharedAccount": ${SHARED_ACCOUNT} }\n`;
}

/**
 * Throws when `text` is not a schema-1 manifest declaring the shared account. Used by the unit
 * test on the generated content, and available to anything that reads the file back.
 * @param {string} text
 */
export function assertLauncherManifest(text) {
  if (!text.endsWith('\n')) throw new Error(`${LAUNCHER_MANIFEST_NAME} must be newline-terminated`);
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(`${LAUNCHER_MANIFEST_NAME} is not valid JSON: ${error.message}`);
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error(`${LAUNCHER_MANIFEST_NAME} must be a JSON object`);
  }
  if (parsed.schema !== LAUNCHER_MANIFEST_SCHEMA) {
    throw new Error(`${LAUNCHER_MANIFEST_NAME} schema must be ${LAUNCHER_MANIFEST_SCHEMA}, got ${JSON.stringify(parsed.schema)}`);
  }
  if (parsed.sharedAccount !== true) {
    throw new Error(`${LAUNCHER_MANIFEST_NAME} must declare "sharedAccount": true, got ${JSON.stringify(parsed.sharedAccount)}`);
  }
  return parsed;
}

/**
 * Throws when a release's asset list does not carry the manifest. A release without it still
 * installs, but the launcher falls back to its built-in table and may tell the user this
 * release needs its own sign-in -- which is the silent failure this check exists to catch.
 * @param {{ name: string }[]} assets  the release's assets, as the GitHub API lists them
 * @param {string} tag                 for the message only
 */
export function assertReleaseCarriesLauncherManifest(assets, tag) {
  if (!assets.some((asset) => asset.name === LAUNCHER_MANIFEST_NAME)) {
    throw new Error(
      `${LAUNCHER_MANIFEST_NAME} is not attached to ${tag}; Redrob Launcher would not learn this release reads the shared account`,
    );
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [, , outputPath] = process.argv;
  if (!outputPath) {
    console.error('usage: launcher-release-manifest.mjs <output-path>');
    process.exit(2);
  }
  const content = launcherManifestContent();
  assertLauncherManifest(content);
  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, content, 'utf8');
  console.log(`wrote ${LAUNCHER_MANIFEST_NAME} to ${outputPath}: ${content.trim()}`);
}
