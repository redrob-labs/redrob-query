import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { isReleaseFile, PAYLOAD_ARCH, stagedName, stageReleaseAssets } from './stage-release-assets.mjs';

const LINUX_TARGET = 'x86_64-unknown-linux-gnu';
const ARM_MAC = 'aarch64-apple-darwin';
const INTEL_MAC = 'x86_64-apple-darwin';
const WINDOWS_TARGET = 'x86_64-pc-windows-msvc';

// What tauri's bundler leaves under target/<triple>/release/bundle on each leg, by directory.
const LINUX = {
  appimage: ['Redrob Query_0.2.4_amd64.AppImage', 'Redrob Query_0.2.4_amd64.AppImage.sig'],
  deb: ['Redrob Query_0.2.4_amd64.deb', 'Redrob Query_0.2.4_amd64.deb.sig', 'Redrob Query_0.2.4_amd64/control.tar.gz'],
};
const MACOS = {
  dmg: ['Redrob Query_0.2.4_aarch64.dmg', 'bundle_dmg.sh'],
  macos: [
    // tauri names the updater payload with neither version nor architecture.
    'Redrob Query.app.tar.gz',
    'Redrob Query.app.tar.gz.sig',
    'Redrob Query.app/Contents/MacOS/Redrob Query',
    'Redrob Query.app/Contents/Info.plist',
  ],
};
const WINDOWS = {
  nsis: ['Redrob Query_0.2.4_x64-setup.exe', 'Redrob Query_0.2.4_x64-setup.exe.sig', 'Redrob Query_0.2.4_x64-setup.nsi'],
  msi: ['Redrob Query_0.2.4_x64_en-US.msi', 'Redrob Query_0.2.4_x64_en-US.msi.sig', 'main.wixobj'],
};

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function bundle(tree: Record<string, string[]>) {
  const root = mkdtempSync(join(tmpdir(), 'redrob-query-bundle-'));
  dirs.push(root);
  for (const [dir, files] of Object.entries(tree)) {
    for (const file of files) {
      const path = join(root, dir, file);
      mkdirSync(join(path, '..'), { recursive: true });
      writeFileSync(path, `bytes of ${file}`);
    }
  }
  return { bundleDir: root, outDir: join(root, 'out') };
}

describe('isReleaseFile', () => {
  it('takes installers, updater payloads and their signatures', () => {
    for (const name of [
      'Redrob Query_0.2.4_amd64.AppImage',
      'Redrob Query_0.2.4_amd64.deb.sig',
      'Redrob Query_0.2.4_aarch64.dmg',
      'Redrob Query.app.tar.gz',
      'Redrob Query_0.2.4_x64-setup.exe.sig',
      'Redrob Query_0.2.4_x64_en-US.msi',
    ]) {
      expect(isReleaseFile(name), name).toBe(true);
    }
  });

  it('leaves the bundler scaffolding behind', () => {
    for (const name of ['bundle_dmg.sh', 'control.tar.gz', 'Redrob Query_0.2.4_x64-setup.nsi', 'main.wixobj', 'Info.plist', 'latest.json']) {
      expect(isReleaseFile(name), name).toBe(false);
    }
  });
});

describe('stagedName', () => {
  it('renames installers and their .sig files to the launcher-readable name', () => {
    expect(stagedName('Redrob Query_0.2.4_amd64.AppImage', '0.2.4', LINUX_TARGET)).toBe('redrob-query-0.2.4-linux-x86_64.AppImage');
    expect(stagedName('Redrob Query_0.2.4_x64_en-US.msi.sig', '0.2.4', WINDOWS_TARGET)).toBe('redrob-query-0.2.4-windows-x64.msi.sig');
    expect(stagedName('Redrob Query_0.2.4_x64-setup.exe', '0.2.4', WINDOWS_TARGET)).toBe('redrob-query-0.2.4-windows-x64-setup.exe');
  });

  it('gives the macOS updater payload the version and launcher os-arch token tauri leaves out', () => {
    expect(stagedName('Redrob Query.app.tar.gz', '0.2.4', ARM_MAC)).toBe('redrob-query-0.2.4-macos-aarch64.app.tar.gz');
    expect(stagedName('Redrob Query.app.tar.gz.sig', '0.2.4', INTEL_MAC)).toBe('redrob-query-0.2.4-macos-x86_64.app.tar.gz.sig');
    expect(PAYLOAD_ARCH).toEqual({ 'aarch64-apple-darwin': 'macos-aarch64', 'x86_64-apple-darwin': 'macos-x86_64' });
  });

  it('refuses an updater payload from a target it has no architecture token for', () => {
    expect(() => stagedName('Redrob Query.app.tar.gz', '0.2.4', 'universal-apple-darwin')).toThrow(/universal-apple-darwin/);
  });

  it('gives any other versioned bundler file the name GitHub would store it under: spaces become dots', () => {
    expect(stagedName('Redrob Query_0.2.4_amd64.rpm', '0.2.4', LINUX_TARGET)).toBe('Redrob.Query_0.2.4_amd64.rpm');
  });

  it('refuses a file built for another version', () => {
    expect(() => stagedName('Redrob Query_0.2.3_amd64.deb', '0.2.4', LINUX_TARGET)).toThrow(/0\.2\.3/);
    expect(() => stagedName('Redrob Query_0.2.3_x64.app.tar.gz', '0.2.4', INTEL_MAC)).toThrow(/0\.2\.3/);
  });

  it('keeps a numeric pre-release identifier, the form the dry run uses', () => {
    expect(stagedName('Redrob Query_0.3.0-1_x64_en-US.msi', '0.3.0-1', WINDOWS_TARGET)).toBe('redrob-query-0.3.0-1-windows-x64.msi');
    expect(stagedName('Redrob Query.app.tar.gz', '0.3.0-1', ARM_MAC)).toBe('redrob-query-0.3.0-1-macos-aarch64.app.tar.gz');
  });
});

describe('stageReleaseAssets', () => {
  it('stages the Linux leg: installers renamed, scaffolding left behind', async () => {
    const { bundleDir, outDir } = bundle(LINUX);
    const names = await stageReleaseAssets(bundleDir, outDir, '0.2.4', LINUX_TARGET);
    expect(names).toEqual([
      'redrob-query-0.2.4-linux-x86_64.AppImage',
      'redrob-query-0.2.4-linux-x86_64.AppImage.sig',
      'redrob-query-0.2.4-linux-x86_64.deb',
      'redrob-query-0.2.4-linux-x86_64.deb.sig',
    ]);
    expect(readdirSync(outDir).sort()).toEqual(names);
  });

  it('stages the macOS leg without descending into the unpacked .app', async () => {
    const { bundleDir, outDir } = bundle(MACOS);
    expect(await stageReleaseAssets(bundleDir, outDir, '0.2.4', ARM_MAC)).toEqual([
      'redrob-query-0.2.4-macos-aarch64.app.tar.gz',
      'redrob-query-0.2.4-macos-aarch64.app.tar.gz.sig',
      'redrob-query-0.2.4-macos-aarch64.dmg',
    ]);
  });

  it('stages the two macOS legs under names that do not collide when the artifacts are merged', async () => {
    const arm = bundle(MACOS);
    const intel = bundle({ ...MACOS, dmg: ['Redrob Query_0.2.4_x64.dmg'] });
    const armNames = await stageReleaseAssets(arm.bundleDir, arm.outDir, '0.2.4', ARM_MAC);
    const intelNames = await stageReleaseAssets(intel.bundleDir, intel.outDir, '0.2.4', INTEL_MAC);
    expect(intelNames).toEqual([
      'redrob-query-0.2.4-macos-x86_64.app.tar.gz',
      'redrob-query-0.2.4-macos-x86_64.app.tar.gz.sig',
      'redrob-query-0.2.4-macos-x86_64.dmg',
    ]);
    expect(armNames.filter((name) => intelNames.includes(name))).toEqual([]);
  });

  it('stages the Windows leg: both installers and both signatures', async () => {
    const { bundleDir, outDir } = bundle(WINDOWS);
    expect(await stageReleaseAssets(bundleDir, outDir, '0.2.4', WINDOWS_TARGET)).toEqual([
      'redrob-query-0.2.4-windows-x64-setup.exe',
      'redrob-query-0.2.4-windows-x64-setup.exe.sig',
      'redrob-query-0.2.4-windows-x64.msi',
      'redrob-query-0.2.4-windows-x64.msi.sig',
    ]);
  });

  it('fails a bundle directory with nothing to release', async () => {
    const { bundleDir, outDir } = bundle({ msi: ['main.wixobj'] });
    await expect(stageReleaseAssets(bundleDir, outDir, '0.2.4', WINDOWS_TARGET)).rejects.toThrow(/no release file/);
  });

  it('fails an installer the launcher could not read instead of shipping it', async () => {
    // A leg whose arch is not in the rename table: the file keeps a bundler name.
    const { bundleDir, outDir } = bundle({ dmg: ['Redrob Query_0.2.4_universal.dmg'] });
    await expect(stageReleaseAssets(bundleDir, outDir, '0.2.4', ARM_MAC)).rejects.toThrow(/Launcher cannot read/);
  });

  it('fails when two files would publish under one name', async () => {
    const { bundleDir, outDir } = bundle({
      a: ['Redrob Query_0.2.4_amd64.AppImage'],
      b: ['Redrob.Query_0.2.4_amd64.AppImage'],
    });
    await expect(stageReleaseAssets(bundleDir, outDir, '0.2.4', LINUX_TARGET)).rejects.toThrow(/both be published/);
  });
});
