import { execFileSync } from 'node:child_process';
import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { assertUpdaterContract, assetUrl, buildUpdaterManifest, UPDATER_KEYS } from './write-updater-manifest.mjs';

const REPO = 'redrob-labs/redrob-query';
const V = '0.2.4';

// The staged names every leg produces for one release (stage-release-assets.mjs).
const STAGED = [
  'redrob-query-0.2.4-linux-x86_64.AppImage',
  'redrob-query-0.2.4-linux-x86_64.deb',
  'Redrob.Query_0.2.4_aarch64.app.tar.gz',
  'redrob-query-0.2.4-macos-aarch64.dmg',
  'Redrob.Query_0.2.4_x64.app.tar.gz',
  'redrob-query-0.2.4-macos-x86_64.dmg',
  'redrob-query-0.2.4-windows-x64-setup.exe',
  'redrob-query-0.2.4-windows-x64.msi',
];
const SIGNED = STAGED.filter((name) => !name.endsWith('.dmg'));
const PLATFORMS = ['linux-x86_64', 'darwin-aarch64', 'darwin-x86_64', 'windows-x86_64'];
const SUFFIXES = ['.AppImage', '.deb', '.dmg', '.exe', '.msi'];

const signed = (names: string[]) => names.map((name) => ({ name, signature: `sig-of-${name}` }));

describe('buildUpdaterManifest', () => {
  it('writes the keys tauri-action wrote for this app, so installed clients keep resolving theirs', () => {
    const manifest = buildUpdaterManifest({ signed: signed(SIGNED), version: V, repository: REPO, pubDate: new Date(0) });
    expect(manifest.version).toBe(V);
    expect(manifest.pub_date).toBe('1970-01-01T00:00:00.000Z');
    expect(Object.keys(manifest.platforms).sort()).toEqual(
      [
        'linux-x86_64', 'linux-x86_64-appimage', 'linux-x86_64-deb',
        'darwin-aarch64', 'darwin-aarch64-app', 'darwin-x86_64', 'darwin-x86_64-app',
        'windows-x86_64', 'windows-x86_64-nsis', 'windows-x86_64-msi',
      ].sort(),
    );
  });

  it('points every URL at this repository release of this version, under the staged name', () => {
    const manifest = buildUpdaterManifest({ signed: signed(SIGNED), version: V, repository: REPO });
    expect(manifest.platforms['linux-x86_64']).toEqual({
      signature: 'sig-of-redrob-query-0.2.4-linux-x86_64.AppImage',
      url: `https://github.com/${REPO}/releases/download/v0.2.4/redrob-query-0.2.4-linux-x86_64.AppImage`,
    });
    expect(manifest.platforms['darwin-x86_64'].url).toBe(assetUrl(REPO, V, 'Redrob.Query_0.2.4_x64.app.tar.gz'));
    // Windows prefers the NSIS installer, as updaterJsonPreferNsis did.
    expect(manifest.platforms['windows-x86_64'].url).toMatch(/-windows-x64-setup\.exe$/);
    expect(manifest.platforms['windows-x86_64-msi'].url).toMatch(/-windows-x64\.msi$/);
  });

  it('refuses a signed file no platform key names, instead of dropping it silently', () => {
    expect(() => buildUpdaterManifest({ signed: signed(['redrob-query-0.2.4-linux-aarch64.AppImage']), version: V, repository: REPO })).toThrow(
      /no updater platform key/,
    );
  });

  it('refuses two files claiming one key', () => {
    const twice = signed(['redrob-query-0.2.4-linux-x86_64.AppImage', 'Redrob.Query_0.2.4_amd64.AppImage'.replace('Redrob.Query_0.2.4_amd64', 'other-0.2.4-linux-x86_64')]);
    expect(() => buildUpdaterManifest({ signed: twice, version: V, repository: REPO })).toThrow(/both claim linux-x86_64/);
  });

  it('has a suffix table that only names launcher-style or dotted bundler names', () => {
    for (const { suffix } of UPDATER_KEYS) expect(suffix).not.toMatch(/ /);
  });
});

describe('assertUpdaterContract', () => {
  const full = () => buildUpdaterManifest({ signed: signed(SIGNED), version: V, repository: REPO });
  const check = (manifest: ReturnType<typeof full>, names = STAGED) =>
    assertUpdaterContract(manifest, { names, version: V, repository: REPO, requiredPlatforms: PLATFORMS, requiredSuffixes: SUFFIXES });

  it('passes a complete release', () => {
    expect(() => check(full())).not.toThrow();
  });

  it('fails when a built platform is missing from the manifest', () => {
    const manifest = buildUpdaterManifest({ signed: signed(SIGNED.filter((n) => !n.includes('windows'))), version: V, repository: REPO });
    expect(() => check(manifest)).toThrow(/missing a signed entry for windows-x86_64/);
  });

  it('fails when a built bundle type left no installer', () => {
    expect(() => check(full(), STAGED.filter((n) => !n.endsWith('.msi')))).toThrow(/no staged file ends in \.msi/);
  });

  it('fails a URL outside this repository, or one that names a file that is not staged', () => {
    const foreign = full();
    foreign.platforms['linux-x86_64'].url = 'https://cdn.example.com/redrob-query.AppImage';
    expect(() => check(foreign)).toThrow(/not a release asset of/);
    const unstaged = full();
    unstaged.platforms['linux-x86_64'].url = assetUrl(REPO, V, 'redrob-query-0.2.4-linux-x86_64.rpm');
    expect(() => check(unstaged)).toThrow(/not among the staged files/);
  });

  it('fails a manifest for another version', () => {
    expect(() => check({ ...full(), version: '0.2.3' })).toThrow(/0\.2\.3 is not 0\.2\.4/);
  });
});

// minisign as Tauri writes it: "untrusted comment" line + base64 payload, the whole text wrapped in
// base64 once more. The pre-hashed ("ED") algorithm the updater uses: Ed25519 over BLAKE2b-512.
function minisignKeypair() {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const keyId = randomBytes(8);
  const pubText = `untrusted comment: minisign public key\n${Buffer.concat([Buffer.from('Ed'), keyId, raw]).toString('base64')}\n`;
  const signFile = (bytes: Buffer) => {
    const sig = sign(null, createHash('blake2b512').update(bytes).digest(), privateKey);
    const trusted = 'trusted comment: timestamp:0';
    const global = sign(null, Buffer.concat([sig, Buffer.from(trusted.slice('trusted comment: '.length))]), privateKey);
    const text = `untrusted comment: signature\n${Buffer.concat([Buffer.from('ED'), keyId, sig]).toString('base64')}\n${trusted}\n${global.toString('base64')}\n`;
    return Buffer.from(text).toString('base64');
  };
  return { publicKey: Buffer.from(pubText).toString('base64'), signFile };
}

describe('write-updater-manifest.mjs as the workflow runs it', () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });

  function stagedRelease() {
    const dir = mkdtempSync(join(tmpdir(), 'redrob-query-assets-'));
    dirs.push(dir);
    const { publicKey, signFile } = minisignKeypair();
    for (const name of STAGED) {
      const bytes = Buffer.from(`bytes of ${name}`);
      writeFileSync(join(dir, name), bytes);
      if (SIGNED.includes(name)) writeFileSync(join(dir, `${name}.sig`), signFile(bytes));
    }
    return { dir, publicKey, signFile };
  }

  const run = (dir: string, publicKey: string) =>
    execFileSync(
      process.execPath,
      ['scripts/write-updater-manifest.mjs', dir, V, REPO, join(dir, 'latest.json')],
      {
        encoding: 'utf8',
        stdio: 'pipe',
        env: { ...process.env, TAURI_UPDATER_PUBLIC_KEY: publicKey, REQUIRED_PLATFORMS: PLATFORMS.join(' '), REQUIRED_ASSET_SUFFIXES: SUFFIXES.join(' ') },
      },
    );

  it('writes latest.json once every signature verifies against its file', () => {
    const { dir, publicKey } = stagedRelease();
    const out = run(dir, publicKey);
    expect(out).toMatch(/every signature verified/);
    const manifest = JSON.parse(readFileSync(join(dir, 'latest.json'), 'utf8'));
    expect(manifest.version).toBe(V);
    expect(Object.keys(manifest.platforms)).toHaveLength(10);
    expect(manifest.platforms['linux-x86_64'].signature).toBe(readFileSync(join(dir, 'redrob-query-0.2.4-linux-x86_64.AppImage.sig'), 'utf8'));
  });

  it('refuses a signature over different bytes -- the repacked-AppImage failure', () => {
    const { dir, publicKey } = stagedRelease();
    writeFileSync(join(dir, 'redrob-query-0.2.4-linux-x86_64.AppImage'), 'repacked bytes');
    expect(() => run(dir, publicKey)).toThrow(/does not match these bytes/);
  });

  it('refuses a signature by another key', () => {
    const { dir } = stagedRelease();
    const other = minisignKeypair();
    expect(() => run(dir, other.publicKey)).toThrow(/signed by key/);
  });

  it('refuses a .sig with no file beside it', () => {
    const { dir, publicKey, signFile } = stagedRelease();
    writeFileSync(join(dir, 'redrob-query-0.2.4-linux-aarch64.AppImage.sig'), signFile(Buffer.from('x')));
    expect(() => run(dir, publicKey)).toThrow(/has no redrob-query-0.2.4-linux-aarch64.AppImage beside it/);
  });
});
