#!/usr/bin/env node
// Rewrite latest.json's per-platform URLs into the form clients will actually fetch.
//
// tauri-action uploads the updater manifest while the release is still a DRAFT, and a draft
// asset has no usable public download URL yet -- GitHub only mints
// /releases/download/<tag>/<name> when the release is published. So the action falls back to
// the REST asset endpoint, https://api.github.com/repos/<repo>/releases/assets/<id>, and that
// is what ends up in the manifest.
//
// Shipping that would break the updater: the REST asset endpoint returns the asset's JSON
// metadata unless the caller sends `Accept: application/octet-stream`, which an updater
// fetching a plain URL does not. The published URL is fully determined by the tag and the
// asset's name, so it can be written now, before publication, and be correct after it.
//
// Usage: node scripts/rewrite-updater-urls.mjs <manifest> <release-json> <repo> <tag>

import { readFile, writeFile } from "node:fs/promises";

const [, , manifestPath, releaseJsonPath, repository, tag] = process.argv;
if (!manifestPath || !releaseJsonPath || !repository || !tag) {
  console.error(
    "usage: rewrite-updater-urls.mjs <manifest> <release-json> <owner/repo> <tag>",
  );
  process.exit(2);
}

const [manifest, release] = await Promise.all([
  readFile(manifestPath, "utf8").then(JSON.parse),
  readFile(releaseJsonPath, "utf8").then(JSON.parse),
]);

const namesById = new Map(
  (release.assets ?? []).map((asset) => [String(asset.id), asset.name]),
);
const publishedPrefix = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/`;

const platforms = manifest.platforms ?? {};
const problems = [];
let rewritten = 0;
let alreadyCorrect = 0;

for (const [platform, entry] of Object.entries(platforms)) {
  const url = entry?.url;
  if (typeof url !== "string" || url.length === 0) {
    problems.push(`${platform} has no url`);
    continue;
  }
  if (url.startsWith(publishedPrefix)) {
    alreadyCorrect += 1;
    continue;
  }
  const match = url.match(/\/releases\/assets\/(\d+)$/);
  if (!match) {
    problems.push(`${platform} url is neither a published URL nor a REST asset URL: ${url}`);
    continue;
  }
  const name = namesById.get(match[1]);
  if (!name) {
    problems.push(
      `${platform} references asset id ${match[1]}, which is not attached to ${tag}`,
    );
    continue;
  }
  entry.url = `${publishedPrefix}${encodeURIComponent(name)}`;
  console.log(`  ${platform}: ${name}`);
  rewritten += 1;
}

if (problems.length > 0) {
  console.error(`Cannot rewrite the updater manifest:\n- ${problems.join("\n- ")}`);
  process.exit(1);
}
if (rewritten === 0 && alreadyCorrect === 0) {
  console.error("The updater manifest lists no platforms at all");
  process.exit(1);
}

await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(
  `Rewrote ${rewritten} platform URL(s); ${alreadyCorrect} already pointed at the published path.`,
);
