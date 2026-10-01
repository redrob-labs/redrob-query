// SPDX-License-Identifier: GPL-3.0-or-later
//
// A table's keys and indexes, shown in the product.
//
// This is what the ported dialect readers are FOR. They were written so the plugin host could answer
// an ER diagram's questions, and the same data answers a question the product itself could not:
// query's schema tree shows columns and their types, and nothing about primary keys, foreign keys or
// indexes.
//
// Read-only by design, which is why this panel exists and a transaction toolbar does not. query 0.1 is
// a documented read-only workspace -- mutation commands are not registered with Tauri at all -- so of
// the five Beekeeper roles the porting plan named, the data editor, the transaction toolbar and the
// multi-statement runner have nothing to act on. Reading a table's structure is the one that fits.

import { useEffect, useState } from 'react';

import type { DatabaseKind } from '../domain/types';
import {
  assertDialectSupported,
  readIncomingKeys,
  readIndexes,
  readOutgoingKeys,
  readPrimaryKeys,
  type TableIndex,
  UnsupportedDialectError,
} from '../schema/schemaKeys';
import type { PrimaryKey, TableKey } from '../plugins/protocol';
import { useWorkspace } from '../store/WorkspaceProvider';
import { Icon } from '../ui/Icon';

interface Structure {
  primaryKeys: PrimaryKey[];
  outgoing: TableKey[];
  incoming: TableKey[];
  indexes: TableIndex[];
}

/** A key's columns as one label, so a composite reads as one relationship rather than several. */
const columnLabel = (columns: string | string[]): string =>
  Array.isArray(columns) ? columns.join(', ') : columns;

const qualified = (table: string, schema?: string): string => (schema ? `${schema}.${table}` : table);

export function StructurePanel() {
  const target = useWorkspace((state) => state.structureTarget);
  const bridge = useWorkspace((state) => state.bridge);
  const connections = useWorkspace((state) => state.connections);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const setUi = useWorkspace((state) => state.setUi);

  const [structure, setStructure] = useState<Structure | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const dialect: DatabaseKind | null =
    connections.find((profile) => profile.id === activeConnectionId)?.kind ?? null;

  useEffect(() => {
    if (!target || !activeConnectionId || !dialect) {
      setStructure(null);
      setError(null);
      return;
    }
    // A stale flag rather than an abort: the reads are queries, not cancellable requests, so the
    // correct behaviour is to discard a late answer instead of pretending it was cancelled.
    let stale = false;
    setLoading(true);
    setError(null);

    const run = (sql: string) =>
      bridge.executeQuery({ connectionId: activeConnectionId, query: sql, language: 'sql' });

    void (async () => {
      try {
        assertDialectSupported(dialect);
        const context = { dialect, table: target.table, schema: target.schema };
        // SQLite's incoming-key read needs its sibling table names, because its pragma answers outward
        // only. The other dialects ignore the list.
        const tables =
          dialect === 'sqlite'
            ? (
                await bridge.loadMetadata(activeConnectionId, null).then(async (databases) => {
                  const names: string[] = [];
                  for (const database of databases) {
                    for (const schema of await bridge.loadMetadata(activeConnectionId, database.id)) {
                      for (const child of await bridge.loadMetadata(activeConnectionId, schema.id)) {
                        if (child.kind === 'table' || child.kind === 'view') names.push(child.name);
                      }
                    }
                  }
                  return names;
                })
              )
            : [];

        const [primaryKeys, outgoing, incoming, indexes] = await Promise.all([
          readPrimaryKeys(run, context),
          readOutgoingKeys(run, context),
          readIncomingKeys(run, { ...context, tables }),
          readIndexes(run, context),
        ]);
        if (!stale) setStructure({ primaryKeys, outgoing, incoming, indexes });
      } catch (caught) {
        if (stale) return;
        setStructure(null);
        // An unsupported dialect is a different message from a failure, and saying which is which is
        // the whole reason the reader distinguishes them.
        setError(
          caught instanceof UnsupportedDialectError
            ? caught.message
            : caught instanceof Error
              ? caught.message
              : String(caught),
        );
      } finally {
        if (!stale) setLoading(false);
      }
    })();

    return () => {
      stale = true;
    };
  }, [activeConnectionId, bridge, dialect, target]);

  if (!target) return null;

  return (
    <aside className="structure-panel" data-testid="structure-panel" aria-label="Table structure">
      <header className="structure-header">
        <span className="structure-title">{qualified(target.table, target.schema)}</span>
        <button
          type="button"
          className="structure-close"
          onClick={() => setUi({ structureTarget: null })}
          aria-label="Close table structure"
        >
          <Icon name="close" />
        </button>
      </header>

      {loading ? (
        <p className="structure-state" data-testid="structure-loading">
          Reading the table structure…
        </p>
      ) : error ? (
        <p className="structure-state" data-testid="structure-error" role="status">
          {error}
        </p>
      ) : !structure ? null : (
        <div className="structure-body">
          <section>
            <h3>Primary key</h3>
            {structure.primaryKeys.length === 0 ? (
              <p className="structure-empty">This table has no primary key.</p>
            ) : (
              <ul data-testid="structure-primary-keys">
                {structure.primaryKeys.map((key) => (
                  <li key={key.columnName}>
                    <code>{key.columnName}</code>
                    <span className="structure-note">position {key.position}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section>
            <h3>References</h3>
            {structure.outgoing.length === 0 ? (
              <p className="structure-empty">This table references no other table.</p>
            ) : (
              <ul data-testid="structure-outgoing">
                {structure.outgoing.map((key) => (
                  <li key={`${key.constraintName}-${columnLabel(key.fromColumn)}`}>
                    <code>{columnLabel(key.fromColumn)}</code>
                    <Icon name="arrowRight" />
                    <code>
                      {qualified(key.toTable, key.toSchema || undefined)}.{columnLabel(key.toColumn)}
                    </code>
                    {key.isComposite ? <span className="structure-badge">composite</span> : null}
                    {key.onDelete && key.onDelete !== 'NO ACTION' ? (
                      <span className="structure-note">on delete {key.onDelete.toLowerCase()}</span>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section>
            <h3>Referenced by</h3>
            {structure.incoming.length === 0 ? (
              <p className="structure-empty">No table references this one.</p>
            ) : (
              <ul data-testid="structure-incoming">
                {structure.incoming.map((key) => (
                  <li key={`${key.constraintName}-${key.fromTable}-${columnLabel(key.fromColumn)}`}>
                    <code>
                      {qualified(key.fromTable, key.fromSchema || undefined)}.
                      {columnLabel(key.fromColumn)}
                    </code>
                    {key.isComposite ? <span className="structure-badge">composite</span> : null}
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section>
            <h3>Indexes</h3>
            {structure.indexes.length === 0 ? (
              <p className="structure-empty">This table has no indexes.</p>
            ) : (
              <ul data-testid="structure-indexes">
                {structure.indexes.map((index) => (
                  <li key={index.name}>
                    <code>{index.columns.join(', ')}</code>
                    <span className="structure-note">{index.name}</span>
                    {index.unique ? <span className="structure-badge">unique</span> : null}
                    {index.origin === 'pk' ? <span className="structure-badge">primary</span> : null}
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}
    </aside>
  );
}
