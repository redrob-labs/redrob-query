#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Check docs/compatibility.md: every `parity` row cites a harness, and every citation names a check that
// harness actually makes.
//
// WHY. `parity` is stage 3's output -- the claim that this product matches its authority, as against `native`,
// which claims only that an implementation is here. This repository had NO compatibility matrix at all until
// stage 3 asked for rows to be promoted and there was nothing to promote them in.
//
// Two failures are possible and the second is the worse one:
//
//   * a row claiming parity with nothing to justify it -- a judgement dressed as a measurement;
//   * a row citing a check no harness makes -- a citation that cannot be followed, which looks verified and is
//     not. A reworded label, a deleted check, or a key invented while writing the row all land here.
//
// The citation keys are read from the harnesses themselves (`--list`), not hardcoded, so this cannot drift from
// what they cover. Both list their keys WITHOUT needing a database, which is what lets this guard run in
// `npm test` while the harnesses themselves cannot.

import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

const MATRIX = 'docs/compatibility.md';
const STATUSES = new Set(['planned', 'native', 'adapter', 'parity']);

// harness key prefix -> how to ask that harness what it covers.
const HARNESSES = {
  dialect: ['npx', ['tsx', 'scripts/verify-dialect-sql.mjs', '--list']],
  'upstream-sql': ['node', ['scripts/verify-upstream-sql.mjs', '--list']],
};

const problems = [];

if (!existsSync(MATRIX)) {
  console.error(`error: ${MATRIX} is missing`);
  process.exit(1);
}

// Ask each harness for its keys. A harness that cannot be asked is a failure: without its list every citation
// into it would be unverifiable, and passing anyway would be the silent-pass this repository refuses elsewhere.
const covered = {};
for (const [prefix, [command, args]] of Object.entries(HARNESSES)) {
  try {
    covered[prefix] = new Set(
      execFileSync(command, args, { encoding: 'utf8' })
        .trim()
        .split('\n')
        .map((line) => line.trim())
        .filter(Boolean),
    );
  } catch (error) {
    problems.push(`harness '${prefix}' could not list its coverage: ${error.message.split('\n')[0]}`);
    covered[prefix] = null;
  }
}

const rows = [];
let inFeatureTable = false;
for (const [index, line] of readFileSync(MATRIX, 'utf8').split('\n').entries()) {
  if (line.startsWith('| Domain')) inFeatureTable = true;
  else if (!line.startsWith('|')) inFeatureTable = false;
  if (!line.startsWith('| ') || line.startsWith('| Domain') || /^[|\-\s]+$/.test(line)) continue;
  const cellCount = line.trim().replace(/^\||\|$/g, '').split('|').length;
  if (inFeatureTable && cellCount !== 6 && cellCount !== 7) {
    // Measured: a stray `|` made a row 8 cells wide and it silently left the parse, so the guard passed with
    // one row fewer. Inside the feature table every row must be counted or rejected.
    problems.push(`line ${index + 1}: feature row has ${cellCount} cells; expected 7`);
    continue;
  }
  const cells = line.trim().replace(/^\||\|$/g, '').split('|').map((cell) => cell.trim());
  // A 6-cell row is kept so it can be rejected: a row that drops the Works column must fail, not vanish
  // from the parse and pass.
  if ((cells.length === 6 || cells.length === 7) && STATUSES.has(cells[4]))
    rows.push({ line: index + 1, cells });
}

if (!rows.length) {
  // A parser that finds nothing passes every check vacuously, which is the one outcome this must never call
  // success.
  console.error('error: no matrix rows parsed; the table shape changed and this guard is now blind');
  process.exit(1);
}

const counts = { planned: 0, native: 0, adapter: 0, parity: 0 };
const cited = new Set();
const verifiedNotParity = [];
// `Works` answers a different question from `Status`: `native` meant "the code is here", and the shipped app had
// native rows no button reaches (docs/user-paths.md). `yes` needs a citation of a test that drives the feature
// the way a user does -- `e2e:<path>::<name>`, where <path> exists and contains <name>.
const E2E = /`e2e:([A-Za-z0-9_./-]+)::([A-Za-z0-9_]+)`/g;
let working = 0;

for (const { line, cells } of rows) {
  if (cells.length !== 7) {
    problems.push(`line ${line}: ${cells[0]}/${cells[1].slice(0, 40)} has no Works column`);
    continue;
  }
  const [domain, feature, authority, route, status, evidence, works] = cells;
  counts[status] += 1;
  if (works !== 'yes' && works !== 'no') {
    problems.push(`line ${line}: ${domain}/${feature.slice(0, 40)} Works must be yes or no, not '${works}'`);
  } else if (works === 'yes') {
    working += 1;
    if (status === 'planned') {
      problems.push(`line ${line}: ${domain}/${feature.slice(0, 40)} is planned but claims Works: yes`);
    }
    const e2e = [...evidence.matchAll(E2E)];
    if (!e2e.length) {
      problems.push(
        `line ${line}: ${domain}/${feature.slice(0, 40)} claims Works: yes but cites no \`e2e:<path>::<name>\``,
      );
    }
    for (const [, path, name] of e2e) {
      const target = path; // relative to the repo root, like MATRIX
      if (!existsSync(target)) {
        problems.push(`line ${line}: cites \`e2e:${path}::${name}\` but ${path} does not exist`);
      } else if (!readFileSync(target, 'utf8').includes(name)) {
        problems.push(`line ${line}: cites \`e2e:${path}::${name}\` but ${path} has no ${name}`);
      }
    }
  }
  const keys = [...evidence.matchAll(/`([a-z-]+):([a-z0-9-]+)`/g)].map((match) => [match[1], match[2]]);

  if (status === 'parity' && !keys.length) {
    problems.push(
      `line ${line}: ${domain}/${feature.slice(0, 40)} claims parity but cites no harness. ` +
        '`parity` is a citation, not a judgement',
    );
  }
  if (status !== 'parity' && keys.length) {
    // NOT an error, and this was a correction. The first version rejected evidence on a non-parity row,
    // reasoning "promote it or drop the citation". redrob-recall produced the case that disproves it: its
    // grammar harness verifies that its search grammar is bloop's REDUCED, with three constructs that are
    // its own. That is a real, checkable relationship and it is not equivalence, so the row must stay
    // `native` while carrying its evidence. Reported so nothing hides.
    verifiedNotParity.push(`${domain}/${feature.slice(0, 40)}`);
  }
  for (const [prefix, key] of keys) {
    cited.add(`${prefix}:${key}`);
    if (!(prefix in covered)) {
      problems.push(`line ${line}: cites unknown harness '${prefix}'`);
    } else if (covered[prefix] && !covered[prefix].has(key)) {
      problems.push(
        `line ${line}: cites \`${prefix}:${key}\` but that harness does not make a check by that name`,
      );
    }
  }
  if (!authority) problems.push(`line ${line}: ${domain}/${feature.slice(0, 40)} names no authority`);
  if (!route) problems.push(`line ${line}: ${domain}/${feature.slice(0, 40)} describes no route`);
}

console.log(`compatibility matrix: ${rows.length} rows`);
console.log(`  works ${working} of ${rows.length} (driven end to end by a cited test)`);
for (const status of ['parity', 'native', 'adapter', 'planned']) {
  console.log(`  ${status.padEnd(8)} ${counts[status]}`);
}
// The reverse direction, reported rather than enforced: a check nobody cites may mean a missing row.
for (const [prefix, keys] of Object.entries(covered)) {
  if (!keys) continue;
  const uncited = [...keys].filter((key) => !cited.has(`${prefix}:${key}`));
  console.log(`  ${prefix}: ${keys.size} checks, ${keys.size - uncited.length} cited`);
  if (uncited.length) console.log(`    no row cites: ${uncited.join(', ')}`);
}

if (verifiedNotParity.length) {
  console.log(`  cited without claiming parity: ${verifiedNotParity.join(', ')}`);
}

if (problems.length) {
  console.error('');
  for (const problem of problems) console.error(`error: ${problem}`);
  process.exit(1);
}
console.log('every parity claim cites a check its harness makes');
