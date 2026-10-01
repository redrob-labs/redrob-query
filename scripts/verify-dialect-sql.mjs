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
  mysqlIncomingKeysSql,
  mysqlIndexesSql,
  mysqlOutgoingKeysSql,
  mysqlPrimaryKeysSql,
} from '../src/schema/mysqlKeys.ts';
import {
  postgresIncomingKeysSql,
  postgresIndexesSql,
  postgresOutgoingKeysSql,
  postgresPrimaryKeysSql,
} from '../src/schema/postgresKeys.ts';
import { groupForeignKeyRows, groupIndexRows, groupPrimaryKeyRows } from '../src/schema/schemaKeys.ts';

// The check keys docs/compatibility.md may cite, printed by `--list` for the matrix guard.
const COVERS = ['sqlite-keys', 'postgres-keys', 'mysql-keys'];

if (process.argv[2] === '--list') {
  for (const key of COVERS) console.log(key);
  process.exit(0);
}

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

console.log(failures === 0 ? '\nPostgreSQL checks passed' : `\n${failures} PostgreSQL check(s) failed`);

// ---------------------------------------------------------------------------------------------------
// MySQL / MariaDB
// ---------------------------------------------------------------------------------------------------

const mysqlCli = process.env.REDROB_MYSQL_CLI;
if (!mysqlCli) {
  console.log('\nREDROB_MYSQL_CLI is not set; skipping the MySQL checks.');
  process.exit(failures === 0 ? 0 : 1);
}
const mysqlArgs = (process.env.REDROB_MYSQL_ARGS ?? '').split(/\s+/).filter(Boolean);

/** Run one statement and return rows as objects, via the client's tab-separated batch output. */
const runMysql = (sql) => {
  const out = execFileSync(
    mysqlCli,
    [...mysqlArgs, '-B', '-e', `USE shop; ${sql.trim().replace(/;$/, '')}`],
    { encoding: 'utf8', env: process.env },
  );
  const lines = out.trim().split('\n').filter(Boolean);
  if (lines.length < 2) return [];
  const header = lines[0].split('\t');
  return lines.slice(1).map((line) => {
    const cells = line.split('\t');
    return Object.fromEntries(header.map((name, index) => [name, cells[index] === 'NULL' ? null : cells[index]]));
  });
};

console.log('\n--- MySQL / MariaDB');

const myOutgoing = groupForeignKeyRows(runMysql(mysqlOutgoingKeysSql('orders', 'shop')), 'outgoing');
check('orders declares two foreign keys', myOutgoing.length, 2);
const myComposite = myOutgoing.find((key) => key.isComposite);
check('the two-column key survives as one composite', myComposite?.fromColumn, ['tenant', 'owner_id']);
check('with its ON DELETE action', myComposite?.onDelete, 'RESTRICT');
const mySimple = myOutgoing.find((key) => !key.isComposite);
check('a single-column key stays a string', mySimple?.fromColumn, 'user_id');
check('its ON DELETE is decoded', mySimple?.onDelete, 'CASCADE');
check('its ON UPDATE is decoded', mySimple?.onUpdate, 'SET NULL');
check('and it is named, not numbered', mySimple?.constraintName, 'fk_orders_user');

// The defect this port fixes: upstream's getIncomingKeys joins referential_constraints with no ON
// clause. Measured on this fixture, that returns 16 rows where 4 are correct.
const myIncoming = groupForeignKeyRows(runMysql(mysqlIncomingKeysSql('users', 'shop')), 'incoming');
check('three distinct keys reference users', myIncoming.length, 3);
check('exactly one of them is composite', myIncoming.filter((key) => key.isComposite).length, 1);
check(
  'no row is duplicated by a missing join condition',
  myIncoming.map((key) => key.constraintName).sort(),
  ['fk_orders_tenant_owner', 'fk_orders_user', 'fk_same'],
);

check('a single-column primary key', groupPrimaryKeyRows(runMysql(mysqlPrimaryKeysSql('users', 'shop'))), [
  { columnName: 'id', position: 1 },
]);
check('a compound primary key in order', groupPrimaryKeyRows(runMysql(mysqlPrimaryKeysSql('ledger', 'shop'))), [
  { columnName: 'entry_date', position: 1 },
  { columnName: 'sequence', position: 2 },
]);

const myIndexes = groupIndexRows(runMysql(mysqlIndexesSql('orders', 'shop')));
const myByName = new Map(myIndexes.map((index) => [index.name, index]));
check('a single-column index', myByName.get('idx_orders_user')?.columns, ['user_id']);
check('a two-column unique index, in order', myByName.get('idx_orders_tenant_owner')?.columns, [
  'tenant',
  'owner_id',
]);
check('and it is reported unique', myByName.get('idx_orders_tenant_owner')?.unique, true);
check('the primary key index is marked pk', myByName.get('PRIMARY')?.origin, 'pk');

console.log(failures === 0 ? '\nall dialect SQL checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
