// Fails when a rule sets both an ink and a ground whose tokens are too close to read, in EITHER theme.
//
// Why a check and not a review. The same class can sit on a light ground in one screen and a dark one
// in another -- `.search-submit` is inside the dark onboarding AND inside the light search page -- so a
// colour chosen for one is invisible in the other, and a screenshot only shows the screen you rendered.
// This found seven such pairs in recall and one in query, of which exactly one had been noticed by eye.
//
// 3:1 is the floor here rather than 4.5:1 because these pairs include badges and glyphs, which WCAG
// treats as non-text; the design system's own marks are set where their symbol clears 3.3:1. A rule
// that genuinely needs a lower ratio is decoration and should not be setting an ink at all.
import { readFileSync } from "node:fs";

const THRESHOLD = 3.0;

function srgbToLinear(channel) {
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
}

function luminance(hex) {
  const h = hex.replace("#", "");
  if (h.length !== 6) return null;
  const [r, g, b] = [0, 2, 4].map((i) => srgbToLinear(parseInt(h.slice(i, i + 2), 16) / 255));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a, b) {
  const [la, lb] = [luminance(a), luminance(b)];
  if (la === null || lb === null) return null;
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/**
 * @param {string[]} sheets absolute paths to the app's own stylesheets
 * @param {string} tokensJson absolute path to @redrob-labs/ui's dist/styles/tokens.json
 * @returns {string[]} one message per pair below the threshold
 */
export function findInkOnGroundCollisions(sheets, tokensJson) {
  const resolved = new Map();
  for (const token of JSON.parse(readFileSync(tokensJson, "utf8")).tokens) {
    if (token.kind === "color") resolved.set(token.name, { light: token.light, dark: token.dark });
  }

  // The app's own aliases, so `var(--ink)` resolves to the token it points at.
  const aliases = new Map();
  for (const sheet of sheets) {
    const text = readFileSync(sheet, "utf8");
    for (const [, name, target] of text.matchAll(/^\s*(--[\w-]+):\s*var\((--[\w-]+)\)\s*;/gm)) {
      aliases.set(name.slice(2), target.slice(2));
    }
  }

  const tokenOf = (value) => {
    const found = /^var\((--[\w-]+)\)$/.exec(value.trim());
    if (!found) return null;
    let name = found[1].slice(2);
    const seen = new Set();
    while (aliases.has(name) && !seen.has(name)) {
      seen.add(name);
      name = aliases.get(name);
    }
    return resolved.has(name) ? name : null;
  };

  const problems = [];
  for (const sheet of sheets) {
    const text = readFileSync(sheet, "utf8");
    for (const rule of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      const selector = rule[1].trim().split("\n").pop().trim();
      const ink = /(?<![\w-])color:\s*([^;]+);/.exec(rule[2]);
      const ground = /background(?:-color)?:\s*([^;]+);/.exec(rule[2]);
      if (!ink || !ground) continue;
      const inkToken = tokenOf(ink[1]);
      const groundToken = tokenOf(ground[1]);
      if (!inkToken || !groundToken) continue;
      for (const theme of ["light", "dark"]) {
        const ratio = contrast(resolved.get(inkToken)[theme], resolved.get(groundToken)[theme]);
        if (ratio === null || ratio >= THRESHOLD) continue;
        problems.push(
          `${selector} — ${theme} theme: ${inkToken} on ${groundToken} is ${ratio.toFixed(2)}:1, ` +
            `below ${THRESHOLD}:1. The same class can sit on both grounds, so pick tokens that pair ` +
            `in each: an action fill takes ink-on-brand, a neutral surface takes ink-primary.`,
        );
      }
    }
  }
  return problems;
}
