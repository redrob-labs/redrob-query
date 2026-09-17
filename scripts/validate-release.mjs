import { readFile } from 'node:fs/promises';
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

if (failures.length) {
  console.error(`Release metadata validation failed:\n- ${failures.join('\n- ')}`);
  process.exitCode = 1;
} else {
  console.log(`Release metadata is synchronized for Redrob Query ${packageJson.version}.`);
}
