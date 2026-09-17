import { readFile } from 'node:fs/promises';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => readFile(resolve(root, path), 'utf8');
const [packageText, lockText, cargoText, toolchainText, tauriText] = await Promise.all([
  read('package.json'),
  read('package-lock.json'),
  read('Cargo.toml'),
  read('rust-toolchain.toml'),
  read('src-tauri/tauri.conf.json'),
]);

const packageJson = JSON.parse(packageText);
const packageLock = JSON.parse(lockText);
const tauri = JSON.parse(tauriText);
const cargoVersion = cargoText.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const rustVersion = cargoText.match(/^rust-version\s*=\s*"([^"]+)"/m)?.[1];
const toolchainVersion = toolchainText.match(/^channel\s*=\s*"([^"]+)"/m)?.[1]?.replace(/\.0$/, '');
const failures = [];
const requireValue = (condition, message) => { if (!condition) failures.push(message); };

requireValue(packageJson.name === 'redrob-query', 'package.json name must be redrob-query');
requireValue(packageJson.private === true, 'package.json must remain private');
requireValue(packageJson.engines?.node === '>=22.12.0', 'package.json must require Node.js 22.12.0 or newer');
requireValue(packageJson.license === 'Apache-2.0', 'package.json license must be Apache-2.0');
requireValue(packageJson.version === cargoVersion, 'package.json and Cargo workspace versions must match');
requireValue(packageJson.version === tauri.version, 'package.json and Tauri versions must match');
requireValue(packageJson.version === packageLock.version, 'package.json and package-lock.json versions must match');
requireValue(packageJson.version === packageLock.packages?.['']?.version, 'package-lock root package version must match');
requireValue(rustVersion === toolchainVersion, 'Cargo rust-version and rust-toolchain channel must match');
requireValue(tauri.productName === 'Redrob Query', 'Tauri productName must be Redrob Query');
requireValue(tauri.identifier === 'ai.redrob.query', 'Tauri identifier must be ai.redrob.query');
requireValue(tauri.app?.windows?.[0]?.label === 'main', 'The primary Tauri window label must be main');
requireValue(tauri.build?.devUrl === 'http://127.0.0.1:1420', 'Tauri devUrl must use the loopback address and Vite port 1420');

// tauri::generate_context! panics at COMPILE TIME on any icon that is not RGBA
// ("icon <path> is not RGBA"), so one RGB icon means the desktop app cannot be
// built at all -- which is exactly how this shipped before there was any CI.
// PNG colour type is byte 25 of the file (IHDR): 6 is RGBA, 2 is RGB, 3 palette.
const iconPngs = (dir) =>
  readdirSync(resolve(root, dir), { withFileTypes: true, recursive: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.png'))
    .map((entry) => resolve(entry.parentPath ?? entry.path, entry.name));

const notRgba = iconPngs('src-tauri/icons').filter((file) => readFileSync(file)[25] !== 6);
requireValue(
  notRgba.length === 0,
  `every src-tauri/icons PNG must be RGBA or tauri::generate_context! will not compile; not RGBA: ${notRgba
    .map((file) => file.slice(root.length + 1))
    .join(', ')}`,
);

// The updater feed is a GitHub Releases asset URL, and this workflow was adapted from a
// sibling product. A feed left pointing at the wrong repository does not fail a build or
// an install -- it silently freezes every client of THIS product on its current version,
// or worse, offers it another product's installer. So assert the URL names this repo.
const releaseConfigSource = await read('scripts/prepare-release-config.mjs');
const expectedFeed =
  'https://github.com/redrob-labs/redrob-query/releases/latest/download/latest.json';
requireValue(
  releaseConfigSource.includes(expectedFeed),
  `scripts/prepare-release-config.mjs must set the updater endpoint to ${expectedFeed}`,
);
requireValue(
  !/cdn\.redrob\.ai/.test(releaseConfigSource),
  'scripts/prepare-release-config.mjs must not reference the decommissioned CDN',
);

// A lockfile that DECLARES an optional platform dependency but carries no package block for
// it installs correctly only on the platform it was generated on: `npm ci` skips an optional
// dependency it has no entry for, without a word, and the build then dies much later at the
// point the missing native binary is needed. That is exactly how this product's macOS legs
// failed -- `Cannot find module '@tauri-apps/cli-darwin-arm64'` after a clean dependency
// install and a green Linux build -- and the same gap had silently dropped every esbuild and
// rollup platform binary too, so the frontend could not have been bundled on macOS or
// Windows either. Dependabot rewrites this file constantly, which is when it regresses.
const lockJson = JSON.parse(lockText);
const lockPackages = lockJson.packages ?? {};
const missingPlatformBlocks = [];
for (const [key, meta] of Object.entries(lockPackages)) {
  for (const dependency of Object.keys(meta.optionalDependencies ?? {})) {
    if (`node_modules/${dependency}` in lockPackages) continue;
    missingPlatformBlocks.push(
      `${dependency} (declared by ${key === '' ? 'the root package' : key})`,
    );
  }
}
// fsevents is the one legitimate absence: a macOS-only file WATCHER for dev mode, declared
// as a version range rather than pinned, and no production bundle step needs it.
const unexplainedMissing = missingPlatformBlocks.filter(
  (entry) => !entry.startsWith('fsevents '),
);
requireValue(
  unexplainedMissing.length === 0,
  'package-lock.json declares optional platform dependencies with no package block, so ' +
    `npm ci cannot install them off this platform: ${unexplainedMissing.join(', ')}`,
);

if (failures.length) {
  console.error(`Release metadata validation failed:\n- ${failures.join('\n- ')}`);
  process.exitCode = 1;
} else {
  console.log(`Release metadata is synchronized for Redrob Query ${packageJson.version}.`);
}
