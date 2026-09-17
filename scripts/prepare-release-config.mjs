import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const publicKey = process.env.TAURI_UPDATER_PUBLIC_KEY?.trim();
if (!publicKey)
  throw new Error("TAURI_UPDATER_PUBLIC_KEY is required for a release build.");

// GitHub Releases IS the update channel -- there is no CDN in front of it.
//
// `releases/latest/download/<asset>` resolves to the newest published, non-prerelease
// release, which is what makes the draft-release QA gate work: a draft is not "latest",
// so no installed client can see a version until a human publishes it. Tauri follows
// the redirect to the asset itself.
//
// The tag workflow attaches this manifest with tauri-action's `uploadUpdaterJson`, and
// its verify-release-assets job fails the build if the manifest is missing or if any
// platform URL points outside this repository's own release assets. Without that guard
// the failure is silent: the app installs and simply never updates again.
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

if (process.platform === "win32") {
  const thumbprint = process.env.WIN_CSC_CERTIFICATE_THUMBPRINT?.trim();
  if (!thumbprint) {
    throw new Error(
      "WIN_CSC_CERTIFICATE_THUMBPRINT is required for a Windows release.",
    );
  }
  config.bundle.windows = {
    certificateThumbprint: thumbprint,
    digestAlgorithm: "sha256",
    timestampUrl: "http://timestamp.digicert.com",
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
console.log(`Prepared release configuration for ${process.platform}.`);
