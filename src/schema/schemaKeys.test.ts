// SPDX-License-Identifier: GPL-3.0-or-later
//
// The ported SQL, executed against a real SQLite database.
//
// This is the reason only SQLite was ported. Beekeeper has working key-reading for postgresql, mysql
// and sqlserver too, and porting their SQL is mechanical -- but there is no server for any of them on
// this machine, and SQL that has never run is not ported code, it is a guess that happens to compile.
// node:sqlite is built in, so this dialect gets a real database with real foreign keys.

import { DatabaseSync } from 'node:sqlite';
import { beforeEach, describe, expect, it } from 'vitest';

import type { CellValue, QueryResult } from '../domain/types';
import {
  assertDialectSupported,
  groupForeignKeyRows,
  KEY_READING_DIALECTS,
  quoteSqliteIdentifier,
  readIncomingKeys,
  readIndexes,
  readOutgoingKeys,
  readPrimaryKeys,
  resolveIdentifier,
  UnknownIdentifierError,
  UnsupportedDialectError,
} from './schemaKeys';

let db: DatabaseSync;

/** A RunSql backed by a real SQLite database, shaped like query's QueryResult. */
const run = async (sql: string): Promise<QueryResult> => {
  const rows = db.prepare(sql).all() as Record<string, CellValue>[];
  // node:sqlite returns null-prototype objects; the production path goes through JSON, which does not.
  const plain = rows.map((row) => ({ ...row }));
  return { columns: [], rows: plain, rowCount: plain.length, durationMs: 0 };
};

beforeEach(() => {
  db = new DatabaseSync(':memory:');
  db.exec(`
    CREATE TABLE users (
      id INTEGER PRIMARY KEY,
      tenant TEXT NOT NULL,
      email TEXT,
      UNIQUE (tenant, id)
    );
    CREATE TABLE orders (
      id INTEGER PRIMARY KEY,
      user_id INTEGER REFERENCES users(id) ON DELETE CASCADE ON UPDATE SET NULL,
      tenant TEXT,
      owner_id INTEGER,
      FOREIGN KEY (tenant, owner_id) REFERENCES users(tenant, id)
    );
    CREATE TABLE line_items (
      id INTEGER PRIMARY KEY,
      order_id INTEGER REFERENCES orders(id)
    );
    CREATE TABLE ledger (entry_date TEXT, sequence INTEGER, PRIMARY KEY (entry_date, sequence));
    CREATE INDEX idx_orders_user ON orders(user_id);
    CREATE UNIQUE INDEX idx_orders_tenant_owner ON orders(tenant, owner_id);
  `);
});

describe('SQLite foreign keys, against a real database', () => {
  it('reads a simple outgoing key with its referential actions', async () => {
    const keys = await readOutgoingKeys(run, { dialect: 'sqlite', table: 'line_items' });
    expect(keys).toHaveLength(1);
    expect(keys[0]).toMatchObject({
      isComposite: false,
      fromTable: 'line_items',
      fromColumn: 'order_id',
      toTable: 'orders',
      toColumn: 'id',
    });
  });

  it('keeps a COMPOSITE key as one key rather than two', async () => {
    // The bug this port fixes. Beekeeper's sqlite client maps each pragma row to one key and hardcodes
    // isComposite:false, so this two-column foreign key would arrive as two unrelated single-column
    // keys -- an ER diagram would draw two edges where the schema has one. Its postgres client groups
    // by constraint name, which is the correct treatment, and SQLite's pragma supplies the same
    // grouping key: measured, three rows with two distinct ids.
    const keys = await readOutgoingKeys(run, { dialect: 'sqlite', table: 'orders' });
    expect(keys).toHaveLength(2);

    const composite = keys.find((key) => key.isComposite);
    expect(composite, 'the two-column key must survive as one composite').toBeDefined();
    expect(composite?.fromColumn).toEqual(['tenant', 'owner_id']);
    expect(composite?.toColumn).toEqual(['tenant', 'id']);
    expect(composite?.toTable).toBe('users');

    const simple = keys.find((key) => !key.isComposite);
    expect(simple?.fromColumn).toBe('user_id');
    expect(simple?.onDelete).toBe('CASCADE');
    expect(simple?.onUpdate).toBe('SET NULL');
  });

  it('reads incoming keys by scanning the other tables', async () => {
    // SQLite's pragma only answers outward, so incoming keys are a scan. users is referenced twice by
    // orders -- once simply, once compositely.
    const keys = await readIncomingKeys(run, { dialect: 'sqlite', table: 'users', tables: ['users', 'orders', 'line_items', 'ledger'] });
    expect(keys).toHaveLength(2);
    expect(keys.every((key) => key.toTable === 'users')).toBe(true);
    expect(keys.map((key) => key.fromTable)).toEqual(['orders', 'orders']);
    expect(keys.filter((key) => key.isComposite)).toHaveLength(1);
  });

  it('does not merge two tables first keys into one composite', async () => {
    // pragma ids restart at 0 per table, so without prefixing the constraint id by its owning table,
    // orders' key 0 and line_items' key 0 would group together as a single two-part composite.
    const keys = await readIncomingKeys(run, { dialect: 'sqlite', table: 'orders', tables: ['orders', 'line_items'] });
    expect(keys).toHaveLength(1);
    expect(keys[0].isComposite).toBe(false);
    expect(keys[0].fromTable).toBe('line_items');
  });

  it('returns nothing for a table nobody references', async () => {
    expect(await readIncomingKeys(run, { dialect: 'sqlite', table: 'ledger', tables: ['users', 'orders', 'line_items', 'ledger'] })).toEqual([]);
  });

  it('handles an empty table list without malformed SQL', async () => {
    expect(await readIncomingKeys(run, { dialect: 'sqlite', table: 'users', tables: [] })).toEqual([]);
  });

  it('marks the direction it read', async () => {
    expect((await readOutgoingKeys(run, { dialect: 'sqlite', table: 'line_items' }))[0].direction).toBe('outgoing');
    expect((await readIncomingKeys(run, { dialect: 'sqlite', table: 'orders', tables: ['line_items'] }))[0].direction).toBe('incoming');
  });
});

describe('SQLite primary keys', () => {
  it('reads a single-column key with its position', async () => {
    expect(await readPrimaryKeys(run, { dialect: 'sqlite', table: 'users' })).toEqual([{ columnName: 'id', position: 1 }]);
  });

  it('reads a compound key in declaration order', async () => {
    // pragma_table_info.pk is a 1-based position, not a boolean -- measured: a=1, b=2.
    expect(await readPrimaryKeys(run, { dialect: 'sqlite', table: 'ledger' })).toEqual([
      { columnName: 'entry_date', position: 1 },
      { columnName: 'sequence', position: 2 },
    ]);
  });
});

describe('SQLite indexes', () => {
  it('groups one index per name with its columns in order', async () => {
    const indexes = await readIndexes(run, { dialect: 'sqlite', table: 'orders' });
    const byName = new Map(indexes.map((index) => [index.name, index]));

    expect(byName.get('idx_orders_user')).toMatchObject({ unique: false, columns: ['user_id'] });
    expect(byName.get('idx_orders_tenant_owner')).toMatchObject({
      unique: true,
      columns: ['tenant', 'owner_id'],
    });
  });

  it('reports the UNIQUE constraint index SQLite creates implicitly', async () => {
    // users has UNIQUE (tenant, id), which SQLite implements as an auto index with origin 'u'.
    const origins = (await readIndexes(run, { dialect: 'sqlite', table: 'users' })).map((index) => index.origin);
    expect(origins).toContain('u');
  });
});

describe('identifier safety', () => {
  it('refuses a name the database did not report', () => {
    // What makes interpolation safe without bind parameters. query's QueryRequest carries no parameter
    // channel, so a table name from a plugin would otherwise be pasted into SQL. It is refused by
    // lookup rather than neutralised by escaping.
    expect(() => resolveIdentifier("orders'); DROP TABLE users; --", ['orders', 'users'])).toThrow(
      UnknownIdentifierError,
    );
    expect(resolveIdentifier('orders', ['orders', 'users'])).toBe('orders');
  });

  it('leaves the database intact even if a hostile name reached the SQL', async () => {
    // Belt and braces: the allow-list is the defence, but the quoting must hold on its own.
    await run(`SELECT * FROM pragma_foreign_key_list('orders''); DROP TABLE users; --')`).catch(() => undefined);
    const survived = db.prepare("SELECT count(*) AS c FROM sqlite_master WHERE name = 'users'").get() as {
      c: number;
    };
    expect(survived.c).toBe(1);
  });

  it('doubles an embedded quote when quoting an identifier', () => {
    expect(quoteSqliteIdentifier('we"ird')).toBe('"we""ird"');
  });
});

describe('dialect support', () => {
  it('accepts the dialects it verified against a real server', () => {
    expect(KEY_READING_DIALECTS).toEqual(['sqlite', 'postgresql', 'mysql']);
    for (const kind of ['sqlite', 'postgresql', 'mysql'] as const) {
      expect(() => assertDialectSupported(kind)).not.toThrow();
    }
  });

  it('says MongoDB has no foreign keys, which is not the same as unimplemented', () => {
    expect(() => assertDialectSupported('mongodb')).toThrow(/has no foreign keys/);
  });

  it('says SQL Server is not implemented, and names what is', () => {
    // SQL Server alone now. PostgreSQL and then MySQL each left this list once a real server turned out
    // to run from an extracted .deb -- both "no server on this machine" claims were assumptions. This
    // one was checked rather than assumed: there is no mssql-server package in the distribution's index
    // at all, only client bindings, and no container runtime.
    expect(() => assertDialectSupported('sqlserver')).toThrow(/not implemented for sqlserver/);
    expect(() => assertDialectSupported('sqlserver')).toThrow(/implemented: sqlite, postgresql, mysql/);
  });
});

describe('row grouping, independent of any database', () => {
  it('treats rows sharing a constraint id as one composite', () => {
    const keys = groupForeignKeyRows(
      [
        { constraint_id: '0', part_position: 0, from_table: 't', from_column: 'a', to_table: 'u', to_column: 'x' },
        { constraint_id: '0', part_position: 1, from_table: 't', from_column: 'b', to_table: 'u', to_column: 'y' },
      ],
      'outgoing',
    );
    expect(keys).toHaveLength(1);
    expect(keys[0]).toMatchObject({ isComposite: true, fromColumn: ['a', 'b'], toColumn: ['x', 'y'] });
  });

  it('leaves a single-part key as a plain string, not a one-element array', () => {
    // An ER diagram reading `key.fromColumn` must not have to handle both shapes for a simple key,
    // which is why the upstream postgres client special-cases length 1 and this follows it.
    const keys = groupForeignKeyRows(
      [{ constraint_id: '1', part_position: 0, from_table: 't', from_column: 'a', to_table: 'u', to_column: 'x' }],
      'outgoing',
    );
    expect(keys[0].fromColumn).toBe('a');
    expect(keys[0].isComposite).toBe(false);
  });

  it('drops absent referential actions rather than reporting an empty string', () => {
    const keys = groupForeignKeyRows(
      [{ constraint_id: '1', from_table: 't', from_column: 'a', to_table: 'u', to_column: 'x', on_update: null }],
      'outgoing',
    );
    expect(keys[0].onUpdate).toBeUndefined();
  });

  it('reports the real constraint name when the dialect supplies one', () => {
    // PostgreSQL names every constraint; SQLite's pragma reports only a number. An earlier version
    // reported the grouping id unconditionally, carried over from the SQLite path, so a PostgreSQL
    // consumer looking for `fk_orders_user` got an oid. Caught by running the SQL against a real
    // server, not by this test -- which exists so it stays caught without one.
    const named = groupForeignKeyRows(
      [{ constraint_id: '16401', constraint_name: 'fk_orders_user', from_table: 'orders', from_column: 'user_id', to_table: 'users', to_column: 'id' }],
      'outgoing',
    );
    expect(named[0].constraintName).toBe('fk_orders_user');
  });

  it('falls back to the grouping id when the dialect has no constraint name', () => {
    const unnamed = groupForeignKeyRows(
      [{ constraint_id: '0', from_table: 'orders', from_column: 'user_id', to_table: 'users', to_column: 'id' }],
      'outgoing',
    );
    expect(unnamed[0].constraintName).toBe('0');
  });

  it('keeps two same-named constraints from different tables apart', () => {
    // PostgreSQL scopes a constraint name to its TABLE, so two tables may both have `fk_same`. Upstream
    // groups incoming keys by constraint_name and merges them into one bogus composite; measured on a
    // live server, five rows referencing one table carried three names and four oids. Grouping is by
    // the id, so these stay two simple keys.
    const keys = groupForeignKeyRows(
      [
        { constraint_id: '100', constraint_name: 'fk_same', from_table: 'a', from_column: 'u', to_table: 'users', to_column: 'id' },
        { constraint_id: '200', constraint_name: 'fk_same', from_table: 'b', from_column: 'u', to_table: 'users', to_column: 'id' },
      ],
      'incoming',
    );
    expect(keys).toHaveLength(2);
    expect(keys.every((key) => !key.isComposite)).toBe(true);
    expect(keys.map((key) => key.fromTable)).toEqual(['a', 'b']);
  });
});
