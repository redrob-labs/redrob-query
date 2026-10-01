#!/usr/bin/env node
//
// Refuse any file copied from Beekeeper Studio's commercially-licensed directories.
//
// WHY THIS EXISTS AND WHY IT IS A MACHINE CHECK.
//
// Beekeeper Studio's repository is MIXED licence. LICENSE-COMMERCIAL.md puts every
// file under a `src-commercial` directory under a commercial licence that forbids
// use in a separate project; all 1,817 other files are GPLv3 and are what this
// product ports. Measured at the pinned commit: 55 commercial files, 544 KB.
//
// 55 of 1,872 is far too small a fraction for review to catch reliably. One file
// pasted in a hurry is a licence violation that looks exactly like ordinary work,
// reads correctly, passes every test, and would ship. So the boundary is enforced
// here rather than trusted to attention.
//
// THIS GUARD HANDLES NO COMMERCIAL CONTENT. It compares git blob SHA-1 values,
// taken from the upstream repository's git TREE listing -- which publishes each
// blob's hash without its bytes. Nothing under src-commercial was fetched to build
// the fingerprint, nothing is stored here, and this file could be published without
// disclosing a line of it. A guard that embedded the material it protects against
// would be the very copy it exists to prevent.
//
// Four layers, because each catches a different mistake:
//
//   1. CONTENT. Every tracked source file in this repository is hashed and compared
//      against the 54 recorded hashes. Catches the realistic failure: a verbatim
//      copy. That is what a paste IS at the moment it happens, and this guard runs
//      in `npm run check`, so it fires before any adaptation can disguise it.
//
//   2. PROVENANCE. Every upstream path recorded in UPSTREAM_NOTICES.md's subsystem
//      table is checked for a commercial path. Catches a DECLARED copy -- somebody
//      writing down that they took a commercial file, which review would also
//      catch, but which should never reach review.
//
//   3. WORKING TREE. If a Beekeeper checkout exists under upstream/, it must not
//      have commercial files staged into git. Catches the accident of adding a
//      whole upstream tree to the index.
//
// The empty blob is deliberately absent from the fingerprint. src-commercial/.keep
// is an empty file and every empty file shares its hash; recording it would flag
// every empty file here. Excluded by hash, not by name, so a future empty
// commercial file is still only excluded for being empty.

import { readFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const fingerprintPath = join(root, "docs/beekeeper-commercial-fingerprint.json");

if (!existsSync(fingerprintPath)) {
  console.error("error: docs/beekeeper-commercial-fingerprint.json is missing");
  console.error("       Without it nothing stops a commercial file being copied in.");
  process.exit(1);
}

const fingerprint = JSON.parse(readFileSync(fingerprintPath, "utf8"));
if (!Array.isArray(fingerprint.files) || fingerprint.files.length === 0) {
  console.error("error: the commercial fingerprint records no files; it cannot guard anything");
  process.exit(1);
}

const byHash = new Map(fingerprint.files.map((f) => [f.sha1, f.path]));
const problems = [];

/** git's blob hash: sha1 of "blob <len>\0<bytes>". The same value the tree listing
 * publishes, so an identical file here and there produces an identical hash. */
function gitBlobSha1(bytes) {
  return createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
}

// --- 1. content ---------------------------------------------------------------
// Tracked files only. An untracked file is not in this repository yet; the check
// that matters runs before a commit, and `git ls-files` is what "in the repository"
// means.
let tracked = [];
try {
  tracked = execFileSync("git", ["ls-files", "-z"], { cwd: root, maxBuffer: 64 * 1024 * 1024 })
    .toString()
    .split("\0")
    .filter(Boolean);
} catch {
  console.error("error: could not list tracked files; is this a git checkout?");
  process.exit(1);
}

// The fingerprint itself would not match anything, but skip it and the guard
// explicitly: reasoning about a file that contains hashes is a distraction.
const selfPaths = new Set([
  "docs/beekeeper-commercial-fingerprint.json",
  "scripts/check-upstream-paths.mjs",
]);

let hashed = 0;
for (const rel of tracked) {
  if (selfPaths.has(rel)) continue;
  let bytes;
  try {
    bytes = readFileSync(join(root, rel));
  } catch {
    continue; // deleted or unreadable; not our concern
  }
  // An empty file can never match: the empty blob is excluded from the
  // fingerprint, and skipping here makes that explicit rather than incidental.
  if (bytes.length === 0) continue;
  hashed += 1;
  const sha = gitBlobSha1(bytes);
  const upstream = byHash.get(sha);
  if (upstream) {
    problems.push(
      `${rel} is byte-identical to ${upstream}, which is under Beekeeper Studio's ` +
        `COMMERCIAL licence and may not be used in this project`,
    );
  }
}

// --- 2. provenance ------------------------------------------------------------
// Scoped to the "Which subsystems" table, and that scoping is not fussiness: the
// first run of this guard failed on UPSTREAM_NOTICES.md's own boundary table, the
// one whose whole purpose is to state that src-commercial must never be copied.
// A file that documents a prohibition mentions the prohibited thing. Only the
// subsystem table is a claim about what this repository CONTAINS, so only it is a
// provenance claim.
const noticesPath = join(root, "UPSTREAM_NOTICES.md");
if (existsSync(noticesPath)) {
  const notices = readFileSync(noticesPath, "utf8");
  const lines = notices.split("\n");
  let inSubsystemTable = false;
  for (const [index, line] of lines.entries()) {
    if (/^#{2,4}\s/.test(line)) {
      inSubsystemTable = /which subsystems/i.test(line);
      continue;
    }
    if (!inSubsystemTable) continue;
    if (!line.trimStart().startsWith("|")) continue;
    if (!line.includes("src-commercial")) continue;
    problems.push(
      `UPSTREAM_NOTICES.md line ${index + 1} records a commercial upstream path in the ` +
        `subsystem table; that file may not be ported`,
    );
  }
}

// --- 3. working tree ----------------------------------------------------------
// If somebody fetches Beekeeper into upstream/ to port from it, the tree itself is
// fine -- reading GPLv3 files is the whole point. What must never happen is those
// paths entering git.
for (const rel of tracked) {
  if (/(^|\/)src-commercial(\/|$)/.test(rel)) {
    problems.push(`${rel} is a tracked path under src-commercial and must not be committed`);
  }
}

// --- 4. the ignore rule itself ------------------------------------------------
// Layer 3 catches a commercial path that reached the index. This asserts the rule
// that should stop it getting there. The probe uses a path INSIDE upstream/ rather
// than the directory name, because the rule is written `/upstream/` with a trailing
// slash to match a directory, and `git check-ignore upstream` misses it when the
// directory does not exist locally -- which is exactly the state a fresh clone is
// in, and therefore the state this check has to work in.
for (const probe of [
  "upstream/beekeeper/apps/studio/src-commercial/backend/handlers/awsHandlers.ts",
  "upstream/beekeeper/apps/studio/src/common/appdb/models/user.ts",
]) {
  try {
    execFileSync("git", ["check-ignore", "-q", probe], { cwd: root, stdio: "ignore" });
  } catch {
    problems.push(
      `${probe} is not ignored by git, so an upstream checkout could be committed ` +
        `wholesale -- 284 MB of it, including the 54 commercial files`,
    );
  }
}

if (problems.length > 0) {
  console.error("error: Beekeeper Studio commercial-licence boundary violated");
  for (const p of problems) console.error(`  ${p}`);
  console.error("");
  console.error("Only files OUTSIDE src-commercial are GPLv3 and portable. See");
  console.error("UPSTREAM_NOTICES.md and docs/upstream-sources.toml.");
  process.exit(1);
}

console.log(
  `commercial boundary: clean (${hashed} tracked files hashed against ` +
    `${fingerprint.files.length} commercial blobs at ${fingerprint.commit.slice(0, 12)})`,
);
