// SPDX-License-Identifier: GPL-3.0-or-later
//
// The host answering key requests end to end, against a real SQLite database.
//
// schemaKeys.test.ts proves the SQL. This proves the whole path a plugin actually travels: a
// postMessage request, the dialect check, resolving the plugin's table name against real metadata, the
// statement, and the reply. The two together are what make the ER diagram's relationships real rather
// than plausible.

import { DatabaseSync } from 'node:sqlite';
import { beforeEach, describe, expect, it } from 'vitest';

import type { DataBridge } from '../api/bridge';
import type { CellValue, MetadataNode, QueryRequest, QueryResult } from '../domain/types';
import { PluginHost, type PluginHostContext } from './host';
import type { PrimaryKey, TableKey } from './protocol';

let db: DatabaseSync;
let executed: string[];

/** A bridge whose metadata and query results both come from one real SQLite database. */
const sqliteBridge = (): DataBridge => {
  const tables = () =>
    (db.prepare("SELECT name, type FROM sqlite_master WHERE type IN ('table','view') ORDER BY name").all() as Array<{
      name: string;
      type: string;
    }>).map((row) => ({ ...row }));

  return {
    mode: 'desktop',
    async loadMetadata(_connectionId: string, parentId?: string | null): Promise<MetadataNode[]> {
      if (!parentId) return [{ id: 'db-main', parentId: null, name: 'main', kind: 'database' }];
      if (parentId === 'db-main') return [{ id: 'schema-main', parentId: 'db-main', name: 'main', kind: 'schema' }];
      if (parentId === 'schema-main') {
        return tables().map((row) => ({
          id: `table-${row.name}`,
          parentId: 'schema-main',
          name: row.name,
          kind: row.type === 'view' ? ('view' as const) : ('table' as const),
        }));
      }
      const table = parentId.replace(/^table-/, '');
      return (db.prepare(`SELECT name, type FROM pragma_table_info('${table}')`).all() as Array<{
        name: string;
        type: string;
      }>).map((row) => ({
        id: `${table}-${row.name}`,
        parentId,
        name: row.name,
        kind: 'column' as const,
        dataType: row.type,
      }));
    },
    async executeQuery(request: QueryRequest): Promise<QueryResult> {
      executed.push(request.query);
      const rows = (db.prepare(request.query).all() as Record<string, CellValue>[]).map((row) => ({ ...row }));
      return { columns: [], rows, rowCount: rows.length, durationMs: 0 };
    },
  } as unknown as DataBridge;
};

const mount = (dialect: 'sqlite' | 'sqlserver' = 'sqlite') => {
  const posted: Array<{ id?: string; result?: unknown; error?: string }> = [];
  const pluginWindow = { postMessage: (message: unknown) => posted.push(message as never) };
  const frame = { contentWindow: pluginWindow } as unknown as HTMLIFrameElement;
  const context: PluginHostContext = {
    bridge: sqliteBridge(),
    activeConnectionId: () => 'local-sqlite',
    activeDialect: () => dialect,
    appName: 'Redrob Query',
    appVersion: '0.1.0',
    readViewState: () => null,
    writeViewState: () => {},
  };
  const host = new PluginHost(frame, 'main-view', context);
  host.start();
  return { host, posted, pluginWindow };
};

const ask = async (fixture: ReturnType<typeof mount>, name: string, args?: unknown) => {
  const id = `${name}-${Math.random().toString(36).slice(2)}`;
  window.dispatchEvent(
    new MessageEvent('message', {
      data: { id, name, args },
      source: fixture.pluginWindow as unknown as MessageEventSource,
    }),
  );
  for (let attempt = 0; attempt < 200; attempt += 1) {
    const reply = fixture.posted.find((message) => message.id === id);
    if (reply) return reply;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  return undefined;
};

beforeEach(() => {
  executed = [];
  db = new DatabaseSync(':memory:');
  db.exec(`
    CREATE TABLE users (id INTEGER PRIMARY KEY, tenant TEXT NOT NULL, UNIQUE (tenant, id));
    CREATE TABLE orders (
      id INTEGER PRIMARY KEY,
      user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
      tenant TEXT,
      owner_id INTEGER,
      FOREIGN KEY (tenant, owner_id) REFERENCES users(tenant, id)
    );
    CREATE INDEX idx_orders_user ON orders(user_id);
  `);
});

describe('plugin host reading keys from a real database', () => {
  it('answers getTableKeys with both directions', async () => {
    const fixture = mount();
    const reply = await ask(fixture, 'getTableKeys', { table: 'users', schema: 'main' });
    const keys = reply?.result as TableKey[];

    expect(reply?.error).toBeUndefined();
    // users declares none and is referenced twice by orders.
    expect(keys.filter((key) => key.direction === 'outgoing')).toHaveLength(0);
    expect(keys.filter((key) => key.direction === 'incoming')).toHaveLength(2);
  });

  it('carries a composite relationship through the whole path', async () => {
    const fixture = mount();
    const keys = (await ask(fixture, 'getOutgoingKeys', { table: 'orders' }))?.result as TableKey[];
    const composite = keys.find((key) => key.isComposite);
    expect(composite?.fromColumn).toEqual(['tenant', 'owner_id']);
    expect(composite?.toColumn).toEqual(['tenant', 'id']);
  });

  it('answers getPrimaryKeys and getTableIndexes', async () => {
    const fixture = mount();
    expect((await ask(fixture, 'getPrimaryKeys', { table: 'orders' }))?.result as PrimaryKey[]).toEqual([
      { columnName: 'id', position: 1 },
    ]);
    const indexes = (await ask(fixture, 'getTableIndexes', { table: 'orders' }))?.result as Array<{
      name: string;
      columns: string[];
    }>;
    expect(indexes.map((index) => index.name)).toContain('idx_orders_user');
  });

  it('refuses a table the database never reported, and runs no statement for it', async () => {
    // The security-relevant path. query's QueryRequest has no bind parameters, so a plugin's table
    // name would otherwise be interpolated into SQL; it is refused by lookup instead.
    const fixture = mount();
    const before = executed.length;
    const reply = await ask(fixture, 'getOutgoingKeys', { table: "orders'); DROP TABLE users; --" });

    expect(reply?.error).toContain('not a known table or view');
    expect(executed.length, 'no statement should have been built from it').toBe(before);
    expect(db.prepare("SELECT count(*) AS c FROM sqlite_master WHERE name='users'").get()).toMatchObject({ c: 1 });
  });

  it('requires a table name', async () => {
    expect((await ask(mount(), 'getOutgoingKeys', {}))?.error).toContain('a table name is required');
  });

  it('refuses the same request on a dialect whose SQL is not ported', async () => {
    // SQL Server: PostgreSQL and MySQL are both ported and verified now, so naming either here would
    // be asserting an earlier cycle's state.
    const reply = await ask(mount('sqlserver'), 'getOutgoingKeys', { table: 'orders' });
    expect(reply?.error).toContain('not implemented for sqlserver');
    expect(reply).not.toHaveProperty('result');
  });
});
