import { describe, expect, it } from 'vitest';

import {
  INSTALLABLE_EXTENSIONS,
  isLauncherInstallable,
  launcherAssetName,
  STEM,
} from './launcher-asset-name.mjs';

// The names tauri-action attached to v0.2.0 (GitHub swaps the space in "Redrob Query" for a
// dot), and what each becomes. Every installer must land on a name the launcher reads.
const V020 = {
  'Redrob.Query_0.2.0_amd64.AppImage': 'redrob-query-0.2.0-linux-x86_64.AppImage',
  'Redrob.Query_0.2.0_amd64.AppImage.sig': 'redrob-query-0.2.0-linux-x86_64.AppImage.sig',
  'Redrob.Query_0.2.0_amd64.deb': 'redrob-query-0.2.0-linux-x86_64.deb',
  'Redrob.Query_0.2.0_amd64.deb.sig': 'redrob-query-0.2.0-linux-x86_64.deb.sig',
  'Redrob.Query_0.2.0_aarch64.dmg': 'redrob-query-0.2.0-macos-aarch64.dmg',
  'Redrob.Query_0.2.0_x64.dmg': 'redrob-query-0.2.0-macos-x86_64.dmg',
  'Redrob.Query_0.2.0_x64_en-US.msi': 'redrob-query-0.2.0-windows-x64.msi',
  'Redrob.Query_0.2.0_x64_en-US.msi.sig': 'redrob-query-0.2.0-windows-x64.msi.sig',
  'Redrob.Query_0.2.0_x64-setup.exe': 'redrob-query-0.2.0-windows-x64-setup.exe',
  'Redrob.Query_0.2.0_x64-setup.exe.sig': 'redrob-query-0.2.0-windows-x64-setup.exe.sig',
  'Redrob.Query_0.2.0_aarch64.app.tar.gz': 'redrob-query-0.2.0-macos-aarch64.app.tar.gz',
  'Redrob.Query_0.2.0_aarch64.app.tar.gz.sig': 'redrob-query-0.2.0-macos-aarch64.app.tar.gz.sig',
  'Redrob.Query_0.2.0_x64.app.tar.gz': 'redrob-query-0.2.0-macos-x86_64.app.tar.gz',
  'Redrob.Query_0.2.0_x64.app.tar.gz.sig': 'redrob-query-0.2.0-macos-x86_64.app.tar.gz.sig',
};

// The manifest keeps its name: it is the updater's fixed entry point.
const UNTOUCHED = ['latest.json'];

describe('launcherAssetName', () => {
  it.each(Object.entries(V020))('renames %s', (from, to) => {
    expect(launcherAssetName(from, '0.2.0')).toBe(to);
  });

  it.each(UNTOUCHED)('leaves %s alone', (name) => {
    expect(launcherAssetName(name, '0.2.0')).toBeNull();
  });

  it('accepts the space the bundler writes before GitHub replaces it', () => {
    expect(launcherAssetName('Redrob Query_0.2.0_amd64.deb', '0.2.0')).toBe(
      'redrob-query-0.2.0-linux-x86_64.deb',
    );
  });

  it('is a no-op on an already renamed asset', () => {
    expect(launcherAssetName('redrob-query-0.2.0-linux-x86_64.AppImage', '0.2.0')).toBeNull();
  });

  it('refuses an asset whose version is not the release version', () => {
    expect(() => launcherAssetName('Redrob.Query_0.1.0_amd64.deb', '0.2.0')).toThrow(/0\.1\.0/);
  });

  it('keeps a pre-release version intact', () => {
    expect(launcherAssetName('Redrob.Query_0.3.0-rc.1_x64_en-US.msi', '0.3.0-rc.1')).toBe(
      'redrob-query-0.3.0-rc.1-windows-x64.msi',
    );
  });
});

describe('isLauncherInstallable', () => {
  it('accepts every renamed installer', () => {
    const installers = Object.values(V020).filter(
      (name) => !name.endsWith('.sig') && !name.endsWith('-setup.exe') && !name.endsWith('.app.tar.gz'),
    );
    expect(installers).toHaveLength(5);
    for (const name of installers) expect(isLauncherInstallable(name)).toBe(true);
  });

  it('rejects the bundler names, the exe, the sigs and the updater files', () => {
    for (const name of [...Object.keys(V020), ...UNTOUCHED]) {
      expect(isLauncherInstallable(name)).toBe(false);
    }
    expect(isLauncherInstallable('redrob-query-0.2.0-windows-x64-setup.exe')).toBe(false);
    expect(isLauncherInstallable('redrob-query-0.2.0-linux-x86_64.AppImage.sig')).toBe(false);
  });

  it('rejects tokens the launcher does not look for', () => {
    expect(isLauncherInstallable('redrob-query-0.2.0-linux-amd64.AppImage')).toBe(false);
    expect(isLauncherInstallable('redrob-query-0.2.0-linux-x64.deb')).toBe(false);
    expect(isLauncherInstallable('redrob-query-0.2.0-darwin-aarch64.dmg')).toBe(false);
    expect(isLauncherInstallable('redrob-query-0.2.0-macos-arm64.dmg')).toBe(false);
    expect(isLauncherInstallable('redrob-query-0.2.0-windows-x86_64.msi')).toBe(false);
    expect(isLauncherInstallable('redrob-query-v0.2.0-windows-x64.msi')).toBe(false);
  });

  it('is pinned to the stem the launcher catalog lists for this app', () => {
    expect(STEM).toBe('redrob-query');
    expect(INSTALLABLE_EXTENSIONS).toContain('.msi');
    expect(isLauncherInstallable('redrob-shape-0.2.0-windows-x64.msi')).toBe(false);
  });
});
