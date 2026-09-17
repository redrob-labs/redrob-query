// Emit the release build matrix, and the updater platform keys that must therefore appear in
// latest.json, from ONE list.
//
// Two things drove this out of the workflow file. First, a static matrix cannot skip a single
// leg, and the Windows leg has to be skippable: Authenticode signing needs a certificate that
// is not configured yet, and a leg that fails on every tag makes the whole release red, which
// hides real regressions instead of surfacing them. Second, the verify job used to hardcode
// the four updater platform keys it required, so adding or removing a leg meant editing two
// places and getting a correct gate to fail for the wrong reason.
//
// Windows is SKIPPED rather than built unsigned. An unsigned installer that reaches a user is
// worse than no installer: it trains people to click through the warning.

import { appendFileSync } from 'node:fs';

const legs = [
  {
    label: 'Linux x64',
    platform: 'ubuntu-24.04',
    target: 'x86_64-unknown-linux-gnu',
    bundles: 'appimage,deb',
    updaterPlatforms: ['linux-x86_64'],
  },
  {
    label: 'macOS Apple Silicon',
    platform: 'macos-15',
    target: 'aarch64-apple-darwin',
    bundles: 'app,dmg',
    updaterPlatforms: ['darwin-aarch64'],
  },
  {
    label: 'macOS Intel',
    platform: 'macos-14',
    target: 'x86_64-apple-darwin',
    bundles: 'app,dmg',
    updaterPlatforms: ['darwin-x86_64'],
  },
  {
    label: 'Windows x64',
    platform: 'windows-2025',
    target: 'x86_64-pc-windows-msvc',
    bundles: 'nsis,msi',
    updaterPlatforms: ['windows-x86_64'],
    requiresWindowsSigning: true,
  },
];

// Which file each bundle type leaves on the release. The verify job asserted
// `.AppImage .deb .dmg .exe .msi` literally, so skipping the Windows leg would have made a
// correct gate fail for a reason that has nothing to do with the release being wrong.
const bundleAssetSuffixes = {
  appimage: ['.AppImage'],
  deb: ['.deb'],
  dmg: ['.dmg'],
  // The .app bundle ships as the updater payload, which the signature check already covers.
  app: [],
  nsis: ['.exe'],
  msi: ['.msi'],
};

const windowsSigningReady =
  (process.env.WINDOWS_SIGNING_READY ?? '').trim().toLowerCase() === 'true';

const included = legs.filter((leg) => !leg.requiresWindowsSigning || windowsSigningReady);
const skipped = legs.filter((leg) => !included.includes(leg));

for (const leg of skipped) {
  console.log(
    `::notice title=Platform skipped::${leg.label} is not built because the repository ` +
      'variable WINDOWS_SIGNING_READY is not "true". Set it once a code-signing ' +
      'certificate is in WIN_CSC_LINK; nothing else needs to change.',
  );
}

if (included.length === 0) {
  console.error('Every platform is excluded; there would be nothing to release.');
  process.exit(1);
}

const matrix = included.map(({ label, platform, target, bundles }) => ({
  label,
  platform,
  target,
  bundles,
}));
const platforms = included.flatMap((leg) => leg.updaterPlatforms);
const assetSuffixes = [
  ...new Set(
    included.flatMap((leg) =>
      leg.bundles.split(',').flatMap((bundle) => {
        const suffixes = bundleAssetSuffixes[bundle.trim()];
        if (suffixes === undefined) {
          console.error(`Unknown bundle type "${bundle.trim()}" on ${leg.label}`);
          process.exit(1);
        }
        return suffixes;
      }),
    ),
  ),
];

console.log(`Building: ${included.map((leg) => leg.label).join(', ')}`);
console.log(`Updater platforms required on the draft: ${platforms.join(' ')}`);
console.log(`Asset suffixes required on the draft: ${assetSuffixes.join(' ')}`);

const output = process.env.GITHUB_OUTPUT;
if (output) {
  appendFileSync(output, `matrix=${JSON.stringify(matrix)}\n`);
  appendFileSync(output, `platforms=${platforms.join(' ')}\n`);
  appendFileSync(output, `asset_suffixes=${assetSuffixes.join(' ')}\n`);
  appendFileSync(output, `windows_signing_ready=${windowsSigningReady}\n`);
}
