// Map the file names Tauri's bundler produces onto the names Redrob Launcher installs.
//
// The launcher reads a release asset only when its name is
//     <stem>-<version>-<os>-<arch>.<ext>
// with stem `redrob-query`, the version without a leading "v", and os-arch one of its own
// tokens (linux-x86_64, macos-aarch64, macos-x86_64, windows-x64, ...). Tauri names bundles
// `Redrob Query_<version>_<arch><suffix>` instead -- and GitHub replaces the space with a dot
// on upload -- so `Redrob.Query_0.2.0_amd64.AppImage` is invisible to the launcher.
//
// Renaming is done on the build runner AFTER the bundler and the signer have finished, by
// stage-release-assets.mjs, so the bytes and the updater's signature over them are untouched.
// latest.json is then written from the staged names (write-updater-manifest.mjs), so the updater
// points at exactly the names the publish job uploads.
//
// Installers, the macOS `.app.tar.gz` updater payloads and their detached .sig files are all
// renamed, so every file on a release reads `redrob-query-<version>-<os>-<arch><suffix>` (the
// launcher skips the payloads -- `.app.tar.gz` is not a kind it installs -- but one convention
// across the asset list is what redrob-recall and the other repositories ship). latest.json keeps
// its name: it is the updater's fixed entry point.

export const STEM = 'redrob-query';

// Tauri's `<arch><suffix>` tail -> the launcher token and the extension to publish under.
const TAILS = [
  { arch: 'amd64', suffix: '.AppImage', token: 'linux-x86_64', ext: '.AppImage' },
  { arch: 'amd64', suffix: '.deb', token: 'linux-x86_64', ext: '.deb' },
  { arch: 'aarch64', suffix: '.dmg', token: 'macos-aarch64', ext: '.dmg' },
  { arch: 'x64', suffix: '.dmg', token: 'macos-x86_64', ext: '.dmg' },
  { arch: 'x64', suffix: '_en-US.msi', token: 'windows-x64', ext: '.msi' },
  // Not something the launcher installs (it takes the .msi), renamed so the two Windows
  // files sit next to each other under one stem.
  { arch: 'x64', suffix: '-setup.exe', token: 'windows-x64', ext: '-setup.exe' },
  // The macOS updater payloads, under the arch tauri-action used to put in their names.
  { arch: 'aarch64', suffix: '.app.tar.gz', token: 'macos-aarch64', ext: '.app.tar.gz' },
  { arch: 'x64', suffix: '.app.tar.gz', token: 'macos-x86_64', ext: '.app.tar.gz' },
];

const TAURI_NAME = /^Redrob[. ]Query_(?<version>[^_]+)_(?<tail>.+?)(?<sig>\.sig)?$/;

/**
 * The launcher-readable name for a bundler asset, or null when the asset is not an installer
 * this release renames (updater payloads, latest.json, anything already renamed).
 *
 * @param {string} name   asset name as attached to the release
 * @param {string} version release version without the leading "v"; the asset must carry it
 * @returns {string | null}
 */
export function launcherAssetName(name, version) {
  const match = TAURI_NAME.exec(name);
  if (!match) return null;
  if (match.groups.version !== version) {
    throw new Error(`${name} carries version ${match.groups.version}, not ${version}`);
  }
  const tail = TAILS.find((t) => match.groups.tail === `${t.arch}${t.suffix}`);
  if (!tail) return null;
  return `${STEM}-${version}-${tail.token}${tail.ext}${match.groups.sig ?? ''}`;
}

// What the launcher accepts, mirrored from its parser: <stem>-<version>-<os>-<arch>.<ext> with
// a digit-led version and an extension it installs.
export const INSTALLABLE_EXTENSIONS = ['.msi', '-portable.zip', '.dmg', '.AppImage', '.deb', '.rpm', '.flatpak', '.tar.gz'];
export const LAUNCHER_TOKENS = [
  'linux-x86_64', 'linux-aarch64', 'macos-universal', 'macos-aarch64', 'macos-x86_64',
  'windows-x64', 'windows-arm64', 'windows-x86', 'freebsd-x86_64',
];

/**
 * True when the launcher would install an asset with this name for `stem`.
 * @param {string} name
 * @param {string} stem
 */
export function isLauncherInstallable(name, stem = STEM) {
  const ext = INSTALLABLE_EXTENSIONS.find((e) => name.endsWith(e));
  if (!ext) return false;
  const base = name.slice(0, -ext.length);
  if (!base.startsWith(`${stem}-`)) return false;
  const rest = base.slice(stem.length + 1);
  const token = LAUNCHER_TOKENS.find((t) => rest.endsWith(`-${t}`));
  if (!token) return false;
  const version = rest.slice(0, -(token.length + 1));
  return /^\d/.test(version) && /^[0-9A-Za-z.+-]+$/.test(version);
}
