// Map the file names Tauri's bundler produces onto the names Redrob Launcher installs.
//
// The launcher reads a release asset only when its name is
//     <stem>-<version>-<os>-<arch>.<ext>
// with stem `redrob-query`, the version without a leading "v", and os-arch one of its own
// tokens (linux-x86_64, macos-aarch64, macos-x86_64, windows-x64, ...). Tauri names bundles
// `Redrob Query_<version>_<arch><suffix>` instead -- and GitHub replaces the space with a dot
// on upload -- so `Redrob.Query_0.2.0_amd64.AppImage` is invisible to the launcher.
//
// Renaming is done on the release AFTER the build, by asset id, so the bundler, the signer and
// the updater's signature over the bytes are untouched. The updater does not depend on these
// names: latest.json embeds each signature and is pointed at the renamed assets by
// rewrite-updater-urls.mjs, which resolves asset ids to whatever name they carry at that moment.
//
// Only installers and their detached .sig files are renamed. The `.app.tar.gz` updater payloads
// and latest.json keep the bundler's names: nothing installs them by hand, and the launcher has
// no use for them.

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
