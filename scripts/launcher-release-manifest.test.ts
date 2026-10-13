import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  assertLauncherManifest,
  assertReleaseCarriesLauncherManifest,
  LAUNCHER_MANIFEST_NAME,
  launcherManifestContent,
} from './launcher-release-manifest.mjs';

// The asset list of a release after rename-release-assets.mjs ran, as the GitHub API lists it.
const RELEASE_ASSETS = [
  'redrob-query-0.2.1-linux-x86_64.AppImage',
  'redrob-query-0.2.1-linux-x86_64.AppImage.sig',
  'redrob-query-0.2.1-macos-aarch64.dmg',
  'redrob-query-0.2.1-windows-x64.msi',
  'latest.json',
].map((name, id) => ({ id, name }));

describe('launcherManifestContent', () => {
  it('is the exact bytes the launcher contract shows, newline-terminated', () => {
    expect(launcherManifestContent()).toBe('{ "schema": 1, "sharedAccount": true }\n');
  });

  it('is valid JSON with schema 1 and sharedAccount true', () => {
    const parsed = JSON.parse(launcherManifestContent());
    expect(parsed).toEqual({ schema: 1, sharedAccount: true });
    expect(() => assertLauncherManifest(launcherManifestContent())).not.toThrow();
  });

  it('is named the one file the launcher fetches from a release', () => {
    expect(LAUNCHER_MANIFEST_NAME).toBe('redrob-launcher.json');
  });
});

describe('assertLauncherManifest', () => {
  it('rejects invalid JSON', () => {
    expect(() => assertLauncherManifest('{ schema: 1 }\n')).toThrow(/not valid JSON/);
  });

  it('rejects a schema the launcher does not read as this contract', () => {
    expect(() => assertLauncherManifest('{ "schema": 2, "sharedAccount": true }\n')).toThrow(/schema must be 1/);
    expect(() => assertLauncherManifest('{ "sharedAccount": true }\n')).toThrow(/schema must be 1/);
  });

  it('rejects a manifest that does not declare the shared account', () => {
    expect(() => assertLauncherManifest('{ "schema": 1, "sharedAccount": false }\n')).toThrow(/sharedAccount/);
    expect(() => assertLauncherManifest('{ "schema": 1 }\n')).toThrow(/sharedAccount/);
    expect(() => assertLauncherManifest('{ "schema": 1, "sharedAccount": "true" }\n')).toThrow(/sharedAccount/);
  });

  it('rejects a file without the trailing newline', () => {
    expect(() => assertLauncherManifest('{ "schema": 1, "sharedAccount": true }')).toThrow(/newline/);
  });

  it('rejects non-object JSON', () => {
    expect(() => assertLauncherManifest('[1]\n')).toThrow(/object/);
    expect(() => assertLauncherManifest('null\n')).toThrow(/object/);
  });
});

describe('assertReleaseCarriesLauncherManifest', () => {
  it('fails a release whose assets do not include redrob-launcher.json', () => {
    expect(() => assertReleaseCarriesLauncherManifest(RELEASE_ASSETS, 'v0.2.1')).toThrow(
      /redrob-launcher\.json is not attached to v0\.2\.1/,
    );
  });

  it('passes once the manifest is attached', () => {
    const assets = [...RELEASE_ASSETS, { id: 99, name: LAUNCHER_MANIFEST_NAME }];
    expect(() => assertReleaseCarriesLauncherManifest(assets, 'v0.2.1')).not.toThrow();
  });

  it('does not accept a differently named file', () => {
    const assets = [...RELEASE_ASSETS, { id: 99, name: 'redrob-launcher.json.sig' }, { id: 100, name: 'launcher.json' }];
    expect(() => assertReleaseCarriesLauncherManifest(assets, 'v0.2.1')).toThrow(/not attached/);
  });
});

describe('launcher-release-manifest.mjs as the workflow runs it', () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });

  it('writes the manifest into a release asset directory that does not exist yet', () => {
    const dir = mkdtempSync(join(tmpdir(), 'redrob-query-manifest-'));
    dirs.push(dir);
    const out = join(dir, 'release-assets', LAUNCHER_MANIFEST_NAME);
    execFileSync(process.execPath, ['scripts/launcher-release-manifest.mjs', out], { stdio: 'pipe' });
    const text = readFileSync(out, 'utf8');
    expect(text).toBe(launcherManifestContent());
    expect(assertLauncherManifest(text)).toEqual({ schema: 1, sharedAccount: true });
  });
});
