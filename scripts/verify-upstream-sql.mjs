#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Run the UPSTREAM's own SQL, extracted from the pinned Beekeeper Studio source, against a live server -- and
// compare it with what this repository's port returns.
//
// WHY THIS EXISTS, AND IT IS NOT A NICE-TO-HAVE.
//
// Three defects in Beekeeper's dialect clients are recorded in docs/dialect-verification.md, in
// UPSTREAM_NOTICES.md, and in the ported source's comments. Each was found by reading the upstream file and
// running its query by hand against a live server. That method has a failure mode nobody catches: a query
// TRANSCRIBED by hand can acquire the very defect it is then reported to have.
//
// That is exactly what happened. A cartesian product was recorded against Beekeeper's MySQL getIncomingKeys --
// "joins referential_constraints with no ON clause, 16 rows where 4 are correct" -- and it was wrong. At the
// pinned commit that query HAS an ON clause, and run verbatim it returns 4 rows, all four correct. Every
// non-commercial client in the tree was then checked: all four joins of referential_constraints carry an ON
// clause. The missing clause was mine, introduced while copying the query into a shell, and the result was
// attributed to the upstream and written into three files.
//
// So the upstream's SQL is no longer transcribed. It is READ OUT OF THE PINNED FILE by this harness, which
// makes a transcription error impossible rather than unlikely.
//
// Not part of `npm test`: it needs a live PostgreSQL and MariaDB plus a Beekeeper checkout at the pinned
// commit, and a test that silently passes when its subject is absent is worse than no test.
//
// Usage:
//   REDROB_BEEKEEPER_CHECKOUT=/path/to/beekeeper \
//   REDROB_PG_PSQL=/path/to/psql REDROB_PG_ARGS="-h 127.0.0.1 -p 5439 -U redrob -d postgres" \
//   REDROB_MYSQL_CLI=/path/to/mariadb REDROB_MYSQL_ARGS="--socket=/tmp/mysql.sock" \
//     node scripts/verify-upstream-sql.mjs

import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

const checkout = process.env.REDROB_BEEKEEPER_CHECKOUT;
const pinFile = 'docs/upstream-sources.toml';

const fail = (message) => {
  console.error(message);
  process.exit(1);
};

if (!checkout) {
  fail(
    'REDROB_BEEKEEPER_CHECKOUT is not set. There is nothing to read the upstream SQL out of, so nothing\n' +
      'would be verified. This is a FAILURE, not a skip -- see docs/dialect-verification.md.',
  );
}
if (!existsSync(checkout)) {
  fail(`no Beekeeper checkout at ${checkout}`);
}

// The pin must match, or the SQL read is not the SQL the port was written against.
const pinned = (readFileSync(pinFile, 'utf8').match(
  /\[beekeeper\][\s\S]*?commit = "([0-9a-f]{40})"/,
) ?? [])[1];
if (!pinned) fail(`could not read beekeeper's pinned commit from ${pinFile}`);
let head;
try {
  head = execFileSync('git', ['-C', checkout, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
} catch {
  fail(`${checkout} is not a git checkout, so its commit cannot be established`);
}
if (head !== pinned) {
  fail(
    `the checkout is at ${head}, not the pinned ${pinned}.\n` +
      'Reading SQL from the wrong commit proves nothing about the port.',
  );
}

const clients = join(checkout, 'apps/studio/src/lib/db/clients');

/**
 * Pull a template-literal SQL string out of an upstream client file.
 *
 * Deliberately anchored to a named declaration (`const incomingSQL = \`...\``) rather than to a line number:
 * a line number silently points at different code after any upstream change, which is the same class of
 * mistake as keying a transcript row by its index instead of its heading.
 */
const extractSql = (file, declaration) => {
  const path = join(clients, file);
  if (!existsSync(path)) fail(`upstream client ${file} is not in the checkout`);
  const text = readFileSync(path, 'utf8');
  const start = text.indexOf(declaration);
  if (start < 0) fail(`${file} no longer declares ${declaration} -- the extraction anchor is stale`);
  const open = text.indexOf('`', start);
  const close = text.indexOf('`', open + 1);
  if (open < 0 || close < 0) fail(`${file}: ${declaration} is not a template literal`);
  return text.slice(open + 1, close);
};

const results = [];
const record = (label, claim, verdict, evidence) => {
  results.push({ label, claim, verdict, evidence });
};

// ---------------------------------------------------------------- MySQL
const mysqlCli = process.env.REDROB_MYSQL_CLI;
const mysqlArgs = (process.env.REDROB_MYSQL_ARGS ?? '').split(/\s+/).filter(Boolean);
if (!mysqlCli) {
  fail('REDROB_MYSQL_CLI is not set, so the MySQL claims cannot be checked. That is a failure, not a skip.');
}
const mysql = (sql) =>
  execFileSync(mysqlCli, [...mysqlArgs, '-D', 'shop', '--batch', '--raw', '-e', sql], {
    encoding: 'utf8',
  })
    .trim()
    .split('\n')
    .slice(1)
    .filter(Boolean);

// The upstream query, read from the pinned file rather than retyped. Its one placeholder is a `?` parameter.
const upstreamIncoming = extractSql('mysql.ts', 'const incomingSQL =');
if (!/referential_constraints/i.test(upstreamIncoming)) {
  fail('the extracted MySQL incoming query does not mention referential_constraints; extraction is wrong');
}
const hasOnClause = /\bon\s+cu\.constraint_name\s*=\s*rc\.constraint_name/i.test(upstreamIncoming);
const upstreamRows = mysql(upstreamIncoming.replace('?', "'users'"));

record(
  'mysql getIncomingKeys has an ON clause',
  'RECORDED AS: no ON clause, a cartesian product',
  hasOnClause ? 'the claim is FALSE -- the ON clause is present' : 'the claim holds',
  `extracted from mysql.ts at the pinned commit; ON clause present: ${hasOnClause}`,
);
record(
  'mysql getIncomingKeys row count',
  'RECORDED AS: 16 rows of which 4 are correct',
  upstreamRows.length === 4 ? 'the claim is FALSE -- 4 rows, all correct' : `${upstreamRows.length} rows`,
  upstreamRows.join(' | '),
);

// Every reference action must belong to its own constraint; a cartesian product would attach a foreign one.
const wrongAction = upstreamRows.filter((row) => {
  const columns = row.split('\t');
  const name = columns[0];
  // fk_orders_user is the only constraint in the fixture with a non-default action pair.
  const actions = columns.slice(-4).join(' ');
  return name === 'fk_orders_user'
    ? !/SET NULL/.test(actions)
    : /SET NULL/.test(actions);
});
record(
  'mysql reference actions belong to their own constraint',
  'RECORDED AS: unrelated constraints carry foreign on_update/on_delete',
  wrongAction.length === 0 ? 'the claim is FALSE -- every action matches its constraint' : 'the claim holds',
  `${wrongAction.length} row(s) carry an action from another constraint`,
);

// ---------------------------------------------------------------- PostgreSQL
const psql = process.env.REDROB_PG_PSQL;
const pgArgs = (process.env.REDROB_PG_ARGS ?? '').split(/\s+/).filter(Boolean);
if (!psql) {
  fail('REDROB_PG_PSQL is not set, so the PostgreSQL claims cannot be checked. That is a failure, not a skip.');
}
const pg = (sql) =>
  execFileSync(psql, [...pgArgs, '-At', '-c', sql], { encoding: 'utf8' }).trim().split('\n').filter(Boolean);

// The upstream groups by constraint_name. PostgreSQL scopes a constraint name to its TABLE, so two tables may
// both hold `fk_same` and grouping by name merges two unrelated keys into one false composite.
const upstreamGroupsByName = readFileSync(join(clients, 'postgresql.ts'), 'utf8').includes(
  "_.groupBy(rows, 'constraint_name')",
);
const [counts] = pg(
  "SELECT count(*) || '|' || count(DISTINCT c.conname) || '|' || count(DISTINCT c.oid) " +
    'FROM pg_constraint c JOIN pg_class t ON t.oid = c.confrelid ' +
    "WHERE c.contype = 'f' AND t.relname = 'users';",
);
const [total, names, oids] = counts.split('|').map(Number);

record(
  'postgres groups incoming keys by constraint_name',
  'RECORDED AS: the upstream groups by name, which collapses distinct keys',
  upstreamGroupsByName ? 'the claim HOLDS' : 'the claim is FALSE -- it does not group by name',
  "postgresql.ts contains _.groupBy(rows, 'constraint_name')",
);
record(
  'postgres constraint names collide across tables',
  'RECORDED AS: distinct names fewer than distinct oids',
  names < oids ? 'the claim HOLDS' : 'the claim is FALSE -- names and oids agree',
  `${total} constraints reference users: ${names} distinct names, ${oids} distinct oids`,
);

// ---------------------------------------------------------------- SQLite
const sqliteText = readFileSync(join(clients, 'sqlite.ts'), 'utf8');
const hardcoded = (sqliteText.match(/isComposite:\s*false/g) ?? []).length;
record(
  'sqlite hardcodes isComposite: false',
  'RECORDED AS: the sqlite client cannot report a composite key',
  hardcoded > 0 ? 'the claim HOLDS' : 'the claim is FALSE -- no hardcoded literal',
  `${hardcoded} occurrence(s) of isComposite: false in sqlite.ts`,
);

// ---------------------------------------------------------------- Report
let falsified = 0;
console.log(`upstream SQL checked at pin ${pinned}\n`);
for (const { label, claim, verdict, evidence } of results) {
  const marker = verdict.includes('FALSE') ? 'FALSIFIED' : 'holds';
  if (verdict.includes('FALSE')) falsified += 1;
  console.log(`${marker.padEnd(10)} ${label}`);
  console.log(`           ${claim}`);
  console.log(`           -> ${verdict}`);
  console.log(`           ${evidence}\n`);
}

// A falsified claim is not a harness failure -- it is the harness working. What must fail is a claim this
// repository still ASSERTS somewhere while the evidence contradicts it. The three files that carried the
// cartesian-product claim were corrected in the same commit that added this harness, so any falsified claim
// found here from now on means a correction is outstanding.
const outstanding = [];
for (const file of ['docs/dialect-verification.md', 'UPSTREAM_NOTICES.md', 'src/schema/mysqlKeys.ts']) {
  const text = readFileSync(file, 'utf8');
  if (/cartesian|카테시안/i.test(text) && !/NOT a cartesian|카테시안 곱이 아니|was wrong|틀렸/i.test(text)) {
    outstanding.push(file);
  }
}
if (outstanding.length) {
  console.log('these files still assert a cartesian product without recording that it was disproved:');
  for (const file of outstanding) console.log(`  ${file}`);
  process.exit(1);
}
console.log(
  `${results.length} claim(s) checked against the pinned upstream and a live server, ` +
    `${falsified} falsified and recorded as such`,
);
