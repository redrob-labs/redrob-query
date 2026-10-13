#!/usr/bin/env node
// Rename the installers on a draft release to the names Redrob Launcher installs, then assert
// that every platform the release built has an installable name.
//
// Runs in the verify job BEFORE rewrite-updater-urls.mjs: a rename keeps the asset id, and the
// rewriter resolves ids to names, so the manifest ends up pointing at the renamed files without
// either script knowing about the other. See launcher-asset-name.mjs for the mapping.
//
// Usage: node scripts/rename-release-assets.mjs <release-json> <owner/repo> <tag>
// Env:   GH_TOKEN (for `gh api`)

import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';

import { isLauncherInstallable, launcherAssetName } from './launcher-asset-name.mjs';

const [, , releaseJsonPath, repository, tag] = process.argv;
if (!releaseJsonPath || !repository || !tag) {
  console.error('usage: rename-release-assets.mjs <release-json> <owner/repo> <tag>');
  process.exit(2);
}
if (!/^v\d/.test(tag)) {
  console.error(`tag ${tag} is not v<version>`);
  process.exit(2);
}
const version = tag.slice(1);

const release = JSON.parse(await readFile(releaseJsonPath, 'utf8'));
const assets = release.assets ?? [];

let renamed = 0;
for (const asset of assets) {
  const target = launcherAssetName(asset.name, version);
  if (target === null || target === asset.name) continue;
  if (assets.some((other) => other.name === target)) {
    console.error(`cannot rename ${asset.name}: ${target} is already attached to ${tag}`);
    process.exit(1);
  }
  execFileSync(
    'gh',
    ['api', '-X', 'PATCH', `repos/${repository}/releases/assets/${asset.id}`, '-f', `name=${target}`],
    { stdio: ['ignore', 'ignore', 'inherit'] },
  );
  console.log(`  ${asset.name} -> ${target}`);
  asset.name = target;
  renamed += 1;
}

// One installable name per platform token the release carries an installer for. Judged on the
// final names, so a bundler rename upstream, or a new leg whose arch is not in the table, fails
// here instead of shipping a release the launcher cannot see.
const installable = assets.filter((asset) => isLauncherInstallable(asset.name));
const installersLeft = assets.filter(
  (asset) =>
    !asset.name.endsWith('.sig') &&
    /\.(AppImage|deb|dmg|msi)$/.test(asset.name) &&
    !isLauncherInstallable(asset.name),
);
if (installersLeft.length > 0) {
  console.error(
    `installers the launcher cannot read are attached to ${tag}:\n- ${installersLeft
      .map((a) => a.name)
      .join('\n- ')}`,
  );
  process.exit(1);
}
if (installable.length === 0) {
  console.error(`no launcher-installable asset is attached to ${tag}`);
  process.exit(1);
}

console.log(`Renamed ${renamed} asset(s); ${installable.length} launcher-installable:`);
for (const asset of installable) console.log(`  ${asset.name}`);
