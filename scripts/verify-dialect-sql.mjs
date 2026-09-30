// SPDX-License-Identifier: GPL-3.0-or-later
//
// Run the dialect SQL this repository exports against a REAL server, and check the parsed result.
//
// Not part of `npm test`, deliberately: it needs a live PostgreSQL, which a contributor's machine may
// not have, and a test that silently passes when its subject is absent is worse than no test. It is
// the harness the SQL was verified with, kept so the verification is repeatable rather than a claim in
// a commit message.
//
// Usage:
//   REDROB_PG_PSQL=/path/to/psql REDROB_PG_ARGS="-h 127.0.0.1 -p 5439 -U redrob -d postgres" \
//     node scripts/verify-dialect-sql.mjs
//
// It expects the fixture schema from docs/dialect-verification.md to be loaded.

import { execFileSync } from 'node:child_process';

import {
  postgresIncomingKeysSql,
  postgresIndexesSql,
  postgresOutgoingKeysSql,
  postgresPrimaryKeysSql,
} from '../src/plugins/postgresKeys.ts';
import { groupForeignKeyRows, groupIndexRows, groupPrimaryKeyRows } from '../src/plugins/schemaKeys.ts';

const psql = process.env.REDROB_PG_PSQL;
if (!psql) {
  console.log('REDROB_PG_PSQL is not set; nothing to verify against. See the header for usage.');
  process.exit(0);
}
const args = (process.env.REDROB_PG_ARGS ?? '').split(/\s+/).filter(Boolean);

/** Run one statement and return rows as objects, via psql's JSON aggregation. */
const run = (sql) => {
  const wrapped = `SELECT coalesce(json_agg(t), '[]') FROM (${sql.trim().replace(/;$/, '')}) t`;
  const out = execFileSync(psql, [...args, '-t', '-A', '-c', wrapped], {
    encoding: 'utf8',
    env: process.env,
  });
  return JSON.parse(out.trim());
};

let failures = 0;
const check = (label, actual, expected) => {
  const same = JSON.stringify(actual) === JSON.stringify(expected);
  if (!same) failures += 1;
  console.log(`${same ? 'ok  ' : 'FAIL'}  ${label}`);
  if (!same) {
    console.log(`        expected ${JSON.stringify(expected)}`);
    console.log(`        actual   ${JSON.stringify(actual)}`);
  }
};

// --- Outgoing keys, including a composite.
const outgoing = groupForeignKeyRows(run(postgresOutgoingKeysSql('orders', 'shop')), 'outgoing');
check('orders declares two foreign keys', outgoing.length, 2);

const composite = outgoing.find((key) => key.isComposite);
check('the two-column key survives as one composite', composite?.fromColumn, ['tenant', 'owner_id']);
check('and names both referenced columns', composite?.toColumn, ['tenant', 'id']);
check('with its ON DELETE action', composite?.onDelete, 'RESTRICT');

const simple = outgoing.find((key) => !key.isComposite);
check('a single-column key stays a string', simple?.fromColumn, 'user_id');
check('its ON DELETE is decoded', simple?.onDelete, 'CASCADE');
check('its ON UPDATE is decoded', simple?.onUpdate, 'SET NULL');

// --- Incoming keys: the regression the constraint-name collision creates.
const incoming = groupForeignKeyRows(run(postgresIncomingKeysSql('users', 'shop')), 'incoming');
// shop.orders twice (one simple, one composite), shop.a once, shop.b once. Grouping by conname would
// merge a.fk_same with b.fk_same and report three.
check('four distinct keys reference users', incoming.length, 4);
check('exactly one of them is composite', incoming.filter((key) => key.isComposite).length, 1);
check(
  'the same-named keys from two tables stay separate',
  incoming.filter((key) => key.constraintName === 'fk_same').map((key) => key.fromTable).sort(),
  ['a', 'b'],
);

// --- Cross-schema: archive.snapshots references shop.orders.
const ordersIncoming = groupForeignKeyRows(run(postgresIncomingKeysSql('orders', 'shop')), 'incoming');
check(
  'a reference from another schema is reported with that schema',
  ordersIncoming.find((key) => key.fromTable === 'snapshots')?.fromSchema,
  'archive',
);

// --- Primary keys.
check('a single-column primary key', groupPrimaryKeyRows(run(postgresPrimaryKeysSql('users', 'shop'))), [
  { columnName: 'id', position: 1 },
]);
check('a compound primary key in order', groupPrimaryKeyRows(run(postgresPrimaryKeysSql('ledger', 'shop'))), [
  { columnName: 'entry_date', position: 1 },
  { columnName: 'sequence', position: 2 },
]);

// --- Indexes. The two-column one is what catches an off-by-one in indkey's 0-based subscripts.
const indexes = groupIndexRows(run(postgresIndexesSql('orders', 'shop')));
const byName = new Map(indexes.map((index) => [index.name, index]));
check('a single-column index', byName.get('idx_orders_user')?.columns, ['user_id']);
check('a two-column unique index, in order', byName.get('idx_orders_tenant_owner')?.columns, [
  'tenant',
  'owner_id',
]);
check('and it is reported unique', byName.get('idx_orders_tenant_owner')?.unique, true);
check('the primary key index is marked pk', byName.get('orders_pkey')?.origin, 'pk');

console.log(failures === 0 ? '\nall dialect SQL checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
