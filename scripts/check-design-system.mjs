#!/usr/bin/env node
// Guards the design system against the four ways it drifts back, each of which a build happily passes.
//
//  1. A raw colour literal in CSS. Once one exists the next edit copies it, and the app stops
//     following `data-theme`, because a literal cannot retint.
//  2. A lucide import. 45-icons.md: "Do not import Feather, Lucide (lucide-react), Material or
//     framework icons." The package can come back as a transitive dependency of a UI kit, so the
//     manifest is checked too, not only the source.
//  3. A brand file edited. 10-logo.md forbids recolouring, restretching and effects, and nothing in a
//     build reads a PNG's pixels -- so a nudged gradient ships silently. The hash is the only gate.
//  4. A typeface that is not in the system. A second display face is a second voice.
//
// Reverse-verified: each check was confirmed to FAIL with the defect it names put back.

import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { findInkOnGroundCollisions } from "./ink-on-ground.mjs";

const ROOT = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const failures = [];

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "dist" || entry.startsWith(".")) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

const files = walk(join(ROOT, "src"));
const rel = (f) => relative(ROOT, f);

// 1. Colour literals. The token imports at the top of a sheet are the only place a colour may be
//    named, and they name a file, not a value.
const COLOUR = /#[0-9a-fA-F]{3,8}\b|\brgba?\(\s*\d|:\s*(?:white|black)\b/g;
for (const file of files.filter((f) => f.endsWith(".css"))) {
  readFileSync(file, "utf8")
    .split("\n")
    .forEach((line, i) => {
      if (line.trimStart().startsWith("@import")) return;
      if (line.trimStart().startsWith("*") || line.trimStart().startsWith("/*")) return;
      for (const hit of line.match(COLOUR) ?? []) {
        failures.push(`${rel(file)}:${i + 1} raw colour ${hit} -- use a token from tokens.css`);
      }
    });
}

// 2. Lucide, in source and in the manifest.
for (const file of files) {
  const text = readFileSync(file, "utf8");
  if (/from\s+["']lucide/.test(text)) {
    failures.push(`${rel(file)} imports lucide -- 45-icons.md forbids it; use icons from @redrob-labs/ui`);
  }
}
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
for (const field of ["dependencies", "devDependencies"]) {
  for (const name of Object.keys(pkg[field] ?? {})) {
    if (/lucide|feather-icons|@mui\/icons/.test(name)) {
      failures.push(`package.json ${field} carries ${name} -- 45-icons.md forbids a framework icon set`);
    }
  }
}

// 3. The pinned brand artwork, byte for byte.
const pin = JSON.parse(readFileSync(join(ROOT, "DESIGN_SYSTEM_PIN.json"), "utf8"));
for (const asset of pin.brand_assets) {
  let actual;
  try {
    actual = createHash("sha256").update(readFileSync(join(ROOT, asset.file))).digest("hex");
  } catch {
    failures.push(`${asset.file} is missing -- it is pinned in DESIGN_SYSTEM_PIN.json`);
    continue;
  }
  if (actual !== asset.sha256) {
    failures.push(
      `${asset.file} does not match its pin.\n    expected ${asset.sha256}\n    actual   ${actual}\n` +
        `    10-logo.md: the artwork is never redrawn, restretched, recoloured or otherwise modified.\n` +
        `    Re-copy it from the delivery at ${asset.delivery_path}, or update the pin deliberately.`,
    );
  }
}
if (pin.version !== (pkg.dependencies?.["@redrob-labs/ui"] ?? "").replace(/^[^\d]*/, "")) {
  failures.push(
    `DESIGN_SYSTEM_PIN.json pins @redrob-labs/ui ${pin.version} but package.json asks for ` +
      `${pkg.dependencies?.["@redrob-labs/ui"]} -- the pin records which delivery the values came from`,
  );
}

// 4. Typefaces. Only the system's own font tokens may set a family.
for (const file of files.filter((f) => f.endsWith(".css"))) {
  readFileSync(file, "utf8")
    .split("\n")
    .forEach((line, i) => {
      const m = /font-family:\s*([^;]+);/.exec(line);
      if (!m) return;
      if (/^(inherit|var\(--font-(sans|sans-kr|sans-hi|mono|serif|serif-kr|serif-display|display)\))$/.test(m[1].trim())) return;
      failures.push(
        `${rel(file)}:${i + 1} font-family ${m[1].trim()} -- use var(--font-sans) or another font token`,
      );
    });
}

// 5. Ink that cannot be read on its own ground, in either theme. See ink-on-ground.mjs for why this is
//    a check rather than a review: a class that sits on both grounds is right in one screen and
//    invisible in the other, and a screenshot only shows the screen you rendered.
for (const problem of findInkOnGroundCollisions(
  files.filter((f) => f.endsWith(".css")),
  join(ROOT, "node_modules/@redrob-labs/ui/dist/styles/tokens.json"),
)) {
  failures.push(problem);
}

// 6. A button with a visible label and no icon. The first audit found letters standing in for icons
//    ('i', '‹', '›', '×') and action buttons with none, and nothing failed. Dialog footers and the
//    command-bar trigger stay text-only on purpose, so they are named here rather than exempted by a
//    pattern that would also wave through the next bare button. Menu items are checked too: an
//    `items` entry with a `label` must carry an `icon`.
// A JSX opening tag ends at the first `>` outside any `{...}`: `onClick={() => x}` contains one.
function openingTagEnd(tag) {
  let depth = 0;
  for (let i = 0; i < tag.length; i++) {
    if (tag[i] === "{") depth++;
    else if (tag[i] === "}") depth--;
    else if (tag[i] === ">" && depth === 0) return i;
  }
  return tag.length;
}
// Drop every brace-balanced `{...}` expression, nested ones included, leaving the literal text.
function stripExpressions(text) {
  let out = "";
  let depth = 0;
  for (const ch of text) {
    if (ch === "{") depth++;
    else if (ch === "}") depth = Math.max(0, depth - 1);
    else if (depth === 0) out += ch;
  }
  return out;
}
const TEXT_ONLY_BUTTONS = new Set([
  "Cancel", // dialog footers: AiSettingsModal, Navigator
  "Discard all", // ChangesPanel footer, beside its own staged list
  "Search or run a command ⌘ K", // the command-bar trigger, which is a field, not an action
]);
for (const file of files.filter((f) => f.endsWith(".tsx") && !f.includes(".test."))) {
  const source = readFileSync(file, "utf8");
  for (const m of source.matchAll(/<button\b[\s\S]*?<\/button>/g)) {
    const body = m[0];
    if (/<Icon\b|<Loader\b|<NodeIcon\b/.test(body)) continue;
    const inner = body.slice(openingTagEnd(body) + 1, -"</button>".length);
    const visible = stripExpressions(inner)
      .replace(/<[^>]+>/g, " ")
      .replace(/\s+/g, " ")
      .trim();
    if (!/[A-Za-z\u2039\u203a\u00d7]/.test(visible) || TEXT_ONLY_BUTTONS.has(visible)) continue;
    const line = source.slice(0, m.index).split("\n").length;
    failures.push(`${rel(file)}:${line} button "${visible.slice(0, 40)}" has no icon -- add one from @redrob-labs/ui`);
  }
  for (const m of source.matchAll(/\{\s*id:\s*'[^']+',\s*label:\s*'([^']+)'(?![^}]*\bicon:)[^}]*\}/g)) {
    const line = source.slice(0, m.index).split("\n").length;
    failures.push(`${rel(file)}:${line} menu item "${m[1]}" has no icon`);
  }
}

if (failures.length) {
  console.error(`design system guard: ${failures.length} problem(s)\n`);
  for (const f of failures) console.error("  " + f);
  process.exit(1);
}
console.log("design system guard: clean");
