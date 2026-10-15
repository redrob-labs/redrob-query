import { access, mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const publicKey = process.env.TAURI_UPDATER_PUBLIC_KEY?.trim();
if (!publicKey)
  throw new Error("TAURI_UPDATER_PUBLIC_KEY is required for a release build.");

// GitHub Releases IS the update channel -- there is no CDN in front of it.
//
// `releases/latest/download/<asset>` resolves to the newest published, non-prerelease
// release. The shared publish job (redrob-labs/.github release-publish.yml) publishes a
// tagged build as latest -- or as a prerelease, invisible to this URL, when the version
// carries a `-` -- and a manual dispatch publishes nothing at all. Tauri follows the
// redirect to the asset itself.
//
// The release workflow's updater-manifest job writes latest.json from the staged files and
// fails the build if any platform the matrix built is missing from it or any URL points
// outside this repository's own release assets. Without that guard the failure is silent:
// the app installs and simply never updates again.
//
// scripts/validate-release.mjs asserts this URL names this repository, so a copy of this
// file taken from a sibling product cannot ship a feed pointing at the wrong repo.
const endpoint =
  "https://github.com/redrob-labs/redrob-query/releases/latest/download/latest.json";

const config = {
  bundle: {
    createUpdaterArtifacts: true,
  },
  plugins: {
    updater: {
      pubkey: publicKey,
      endpoints: [endpoint],
      windows: { installMode: "passive" },
    },
  },
};

// A dry run (workflow_dispatch with a version) builds installers for a version the sources do
// not declare, e.g. 0.3.0-rc.1 on a tree that says 0.2.4. The bundle names, the updater
// payloads and latest.json all have to agree on ONE version, so the dispatched one overrides
// the config here. A tag run passes the version the sources already carry (the version job
// refuses the tag otherwise), so this is a no-op there.
const releaseVersion = process.env.RELEASE_VERSION?.trim();
if (releaseVersion) config.version = releaseVersion;

if (process.platform === "win32") {
  // Authenticode through Azure Artifact Signing, with the ONE copy of the recipe the
  // organisation keeps: redrob-labs/.github/actions/sign-windows/sign.ps1, checked out by
  // the workflow at `.redrob-shared`. Tauri signs the executable before packaging and each
  // installer after, so every one of them goes through this command; it reads the
  // ARTIFACT_SIGNING_* variables from the job's environment and authenticates with the
  // `azure/login` the job performed right before `tauri build`. No certificate file, no
  // thumbprint, no client secret anywhere in this repository.
  //
  // Absolute path on purpose: the bundler does not promise which directory it runs the
  // command from, and a relative path that resolves from src-tauri/ fails only on the
  // Windows leg of a real release.
  const sharedDir = resolve(process.env.REDROB_SHARED_DIR?.trim() || ".redrob-shared");
  const signScript = resolve(sharedDir, "actions/sign-windows/sign.ps1");
  try {
    await access(signScript);
  } catch {
    throw new Error(
      `${signScript} is missing: check out redrob-labs/.github at ${sharedDir} before preparing a Windows release.`,
    );
  }
  config.bundle.windows = {
    signCommand: {
      cmd: "pwsh",
      args: ["-NoProfile", "-File", signScript, "%1"],
    },
  };
}

// Written only inside the runner. It carries no private key -- the updater private key
// stays in the signing step's environment -- but it is gitignored so a local run cannot
// leave a release configuration lying in the working tree.
const output = resolve("src-tauri/tauri.release.conf.json");
await mkdir(resolve("src-tauri"), { recursive: true });
await writeFile(output, `${JSON.stringify(config, null, 2)}\n`, {
  mode: 0o600,
});
console.log(
  `Prepared release configuration for ${process.platform}${releaseVersion ? ` (version ${releaseVersion})` : ""}.`,
);
