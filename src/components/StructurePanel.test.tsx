// SPDX-License-Identifier: GPL-3.0-or-later
//
// The structure panel showing real keys from a real database.
//
// The dialect readers were verified in isolation and through the plugin host. This is the third
// consumer and the one the user actually sees: it fails if the panel is wired up anywhere short of the
// store, the bridge and the readers together.

import { DatabaseSync } from 'node:sqlite';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';

import type { DataBridge } from '../api/bridge';
import type { CellValue, ConnectionProfile, MetadataNode, QueryRequest, QueryResult } from '../domain/types';
import { WorkspaceProvider, useWorkspace } from '../store/WorkspaceProvider';
import { StructurePanel } from './StructurePanel';

let db: DatabaseSync;

const profile = (kind: ConnectionProfile['kind']): ConnectionProfile => ({
  id: 'local-sqlite',
  name: 'Local SQLite',
  kind,
  host: '',
  port: 0,
  database: 'main',
  username: '',
  tls: false,
  state: 'connected',
});

/** A bridge whose metadata and query results come from one real SQLite database. */
const sqliteBridge = (kind: ConnectionProfile['kind'] = 'sqlite'): DataBridge =>
  ({
    mode: 'desktop',
    async listConnections() {
      return [profile(kind)];
    },
    async profileWarnings() {
      return [];
    },
    async connect() {
      return { id: 'local-sqlite', state: 'connected' as const };
    },
    async loadMetadata(_connectionId: string, parentId?: string | null): Promise<MetadataNode[]> {
      if (!parentId) return [{ id: 'db-main', parentId: null, name: 'main', kind: 'database', childCount: 1 }];
      if (parentId === 'db-main')
        return [{ id: 'schema-main', parentId: 'db-main', name: 'main', kind: 'schema', childCount: 3 }];
      if (parentId === 'schema-main') {
        return (
          db.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").all() as Array<{
            name: string;
          }>
        ).map((row) => ({
          id: `table-${row.name}`,
          parentId: 'schema-main',
          name: row.name,
          kind: 'table' as const,
          childCount: 1,
        }));
      }
      return [];
    },
    async executeQuery(request: QueryRequest): Promise<QueryResult> {
      const rows = (db.prepare(request.query).all() as Record<string, CellValue>[]).map((row) => ({ ...row }));
      return { columns: [], rows, rowCount: rows.length, durationMs: 0 };
    },
  }) as unknown as DataBridge;

/** Drives the store the way the Navigator does, so the panel is exercised through real state. */
function Harness({ table, schema }: { table?: string; schema?: string }) {
  const setUi = useWorkspace((state) => state.setUi);
  const initialize = useWorkspace((state) => state.initialize);
  const initialized = useWorkspace((state) => state.initialized);
  // A desktop bridge does not auto-select a connection -- only the demo bridge does -- so the harness
  // activates one the way a desktop user does. Discovered by the panel never leaving its loading state.
  const setActiveConnection = useWorkspace((state) => state.setActiveConnection);
  return (
    <>
      <button
        type="button"
        onClick={() => {
          void initialize().then(() => setActiveConnection('local-sqlite'));
        }}
      >
        init
      </button>
      <button
        type="button"
        data-testid="select-table"
        onClick={() => setUi({ structureTarget: table ? { table, schema } : null })}
      >
        select
      </button>
      {initialized ? <StructurePanel /> : null}
    </>
  );
}

const mount = async (table?: string, kind: ConnectionProfile['kind'] = 'sqlite', schema?: string) => {
  const user = userEvent.setup();
  render(
    <WorkspaceProvider bridge={sqliteBridge(kind)}>
      <Harness table={table} schema={schema} />
    </WorkspaceProvider>,
  );
  await user.click(screen.getByText('init'));
  await waitFor(() => expect(screen.getByTestId('select-table')).toBeTruthy());
  // Let the activation settle before selecting a table, or the panel's effect runs with no connection.
  await new Promise((resolve) => setTimeout(resolve, 50));
  await user.click(screen.getByTestId('select-table'));
  return user;
};

beforeEach(() => {
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
    CREATE TABLE ledger (entry_date TEXT, sequence INTEGER, PRIMARY KEY (entry_date, sequence));
    CREATE INDEX idx_orders_user ON orders(user_id);
  `);
});

describe('StructurePanel', () => {
  it('shows nothing until a table is selected', async () => {
    await mount(undefined);
    expect(screen.queryByTestId('structure-panel')).toBeNull();
  });

  it('shows a table primary key, references and indexes', async () => {
    await mount('orders');
    await waitFor(() => expect(screen.getByTestId('structure-primary-keys')).toBeTruthy(), { timeout: 5000 });

    expect(within(screen.getByTestId('structure-primary-keys')).getByText('id')).toBeTruthy();

    const outgoing = screen.getByTestId('structure-outgoing');
    expect(within(outgoing).getByText('user_id')).toBeTruthy();
    expect(outgoing.textContent).toContain('users.id');

    const indexes = screen.getByTestId('structure-indexes');
    expect(indexes.textContent).toContain('idx_orders_user');
  });

  it('labels a composite relationship as one, not several', async () => {
    // The whole reason the readers group rows. A composite shown as two separate references is the
    // upstream behaviour this port deliberately does not reproduce.
    await mount('orders');
    const outgoing = await waitFor(() => screen.getByTestId('structure-outgoing'), { timeout: 5000 });
    expect(within(outgoing).getByText('tenant, owner_id')).toBeTruthy();
    expect(outgoing.textContent).toContain('composite');
  });

  it('shows what references a table, not only what it references', async () => {
    await mount('users');
    const incoming = await waitFor(() => screen.getByTestId('structure-incoming'), { timeout: 5000 });
    expect(incoming.textContent).toContain('orders');
  });

  it('shows a compound primary key in order', async () => {
    await mount('ledger');
    const keys = await waitFor(() => screen.getByTestId('structure-primary-keys'), { timeout: 5000 });
    const columns = Array.from(keys.querySelectorAll('code')).map((node) => node.textContent);
    expect(columns).toEqual(['entry_date', 'sequence']);
  });

  it('says a table has no relationships rather than showing an empty list', async () => {
    await mount('ledger');
    await waitFor(() => expect(screen.getByTestId('structure-panel')).toBeTruthy());
    await waitFor(() => expect(screen.getByText(/references no other table/)).toBeTruthy(), { timeout: 5000 });
    expect(screen.getByText(/No table references this one/)).toBeTruthy();
  });

  it('reports an unsupported dialect instead of failing silently', async () => {
    // SQL Server has no ported SQL, and the panel must say so rather than render four empty sections
    // that look like a table with no keys.
    await mount('orders', 'sqlserver');
    const error = await waitFor(() => screen.getByTestId('structure-error'), { timeout: 5000 });
    expect(error.textContent).toContain('not implemented for sqlserver');
  });

  it('closes on request', async () => {
    const user = await mount('orders');
    await waitFor(() => expect(screen.getByTestId('structure-panel')).toBeTruthy());
    await user.click(screen.getByLabelText('Close table structure'));
    await waitFor(() => expect(screen.queryByTestId('structure-panel')).toBeNull());
  });
});
