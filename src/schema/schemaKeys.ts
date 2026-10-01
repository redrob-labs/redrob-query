// SPDX-License-Identifier: GPL-3.0-or-later
//
// Reading foreign keys, primary keys and indexes, so the plugin host can answer the three methods it
// previously refused. An ER diagram is entirely about relationships, and query's `MetadataNode` has
// none: it carries id, parentId, name, kind, dataType and childCount, and nothing about constraints.
//
// Ported from Beekeeper Studio's dialect clients at the commit pinned in docs/upstream-sources.toml,
// under GPL-3.0-or-later. The port is deliberately not line-for-line, and the three places it departs
// are each a measured improvement rather than a preference.

import type { CellValue, DatabaseKind, QueryResult } from '../domain/types';
import {
  mysqlIncomingKeysSql,
  mysqlIndexesSql,
  mysqlOutgoingKeysSql,
  mysqlPrimaryKeysSql,
} from './mysqlKeys';
import {
  postgresIncomingKeysSql,
  postgresIndexesSql,
  postgresOutgoingKeysSql,
  postgresPrimaryKeysSql,
} from './postgresKeys';
import type { PrimaryKey, TableKey } from '../plugins/protocol';
import { quoteIdentifier, sqlStringLiteral } from './sqlIdentifiers';

/** Runs one statement and returns its rows. Supplied by the host, which owns the connection. */
export type RunSql = (sql: string) => Promise<QueryResult>;

/**
 * Dialects this module can read keys for.
 *
 * SQLite, PostgreSQL and MySQL/MariaDB, each verified against a real running server rather than
 * reasoned about. SQL Server is not here: unlike the other three it is not in the distribution's
 * package index at all, so there is nothing to extract, and SQL that has never executed is not ported
 * code but a guess that happens to compile.
 *
 * Both PostgreSQL and MySQL were written off as unavailable in earlier cycles, and both were wrong:
 * the servers extract from their .deb packages into a private prefix and run as an unprivileged user.
 * That is worth recording twice, because "no server here" was an assumption each time.
 */
export const KEY_READING_DIALECTS: readonly DatabaseKind[] = ['sqlite', 'postgresql', 'mysql'];

/** MongoDB has no foreign keys at all, which is a different answer from "not implemented". */
export const DIALECTS_WITHOUT_KEYS: readonly DatabaseKind[] = ['mongodb'];

export class UnsupportedDialectError extends Error {}
export class UnknownIdentifierError extends Error {}

/**
 * Quote an identifier for SQLite, doubling any embedded quote.
 *
 * Defence in depth, NOT the defence. The real protection is that a caller resolves the name against
 * metadata the database itself reported before it reaches here -- see `resolveIdentifier`. That matters
 * because query's `QueryRequest` carries no bind parameters, only `query: string`, so an identifier
 * arriving from a plugin would otherwise be interpolated into SQL. Beekeeper escapes and interpolates;
 * this refuses anything the database did not name, then quotes it as well.
 */
export const quoteSqliteIdentifier = quoteIdentifier;

/** A single-quoted SQLite string literal, for the pragma functions that take a table NAME as text. */
export const sqliteStringLiteral = sqlStringLiteral;

/**
 * Accept `name` only if the database reported it.
 *
 * This is what makes interpolation safe without bind parameters: the SQL below can only ever contain
 * a name that came out of the schema, so a plugin asking for `orders'); DROP TABLE users; --` is
 * refused by lookup rather than neutralised by escaping. Measured: with a bound parameter that string
 * returns zero rows and harms nothing, but query cannot bind, so the equivalent guarantee has to come
 * from the allow-list.
 */
export const resolveIdentifier = (requested: string, known: readonly string[]): string => {
  const match = known.find((candidate) => candidate === requested);
  if (match === undefined) {
    throw new UnknownIdentifierError(`not a known table or view: ${requested}`);
  }
  return match;
};

const text = (value: CellValue): string => (value === null || value === undefined ? '' : String(value));

const rowsOf = (result: QueryResult): Record<string, CellValue>[] => result.rows ?? [];

/**
 * Group foreign key rows into keys, joining the parts of a composite.
 *
 * THE DEPARTURE FROM UPSTREAM THAT MATTERS. Beekeeper's sqlite client maps each pragma row to one key
 * and hardcodes `isComposite: false`, so a two-column foreign key comes back as two unrelated
 * single-column keys -- an ER diagram would draw two edges where the schema has one. Its postgres
 * client does group, by constraint name, which is the correct treatment; this applies that treatment
 * to SQLite, whose pragma supplies exactly the grouping key needed.
 *
 * Measured on a real database: a table with one composite (two-column) and one simple foreign key
 * yields three pragma rows with two distinct `id` values, ordered by `id` then `seq`.
 */
export const groupForeignKeyRows = (
  rows: readonly Record<string, CellValue>[],
  direction: 'outgoing' | 'incoming',
): TableKey[] => {
  const byConstraint = new Map<string, Record<string, CellValue>[]>();
  for (const row of rows) {
    const key = text(row.constraint_id);
    const parts = byConstraint.get(key);
    if (parts) parts.push(row);
    else byConstraint.set(key, [row]);
  }

  const keys: TableKey[] = [];
  for (const [constraintId, parts] of byConstraint) {
    const first = parts[0];
    const composite = parts.length > 1;
    keys.push({
      // The real name when the dialect has one, the grouping id otherwise. PostgreSQL names every
      // constraint; SQLite's pragma reports only a numeric id, so there is nothing better to use.
      // Reporting the id unconditionally -- which an earlier version did, carried over from the SQLite
      // path -- meant a PostgreSQL consumer looking for `fk_orders_user` found an oid instead.
      constraintName: text(first.constraint_name) || constraintId,
      isComposite: composite,
      fromTable: text(first.from_table),
      fromSchema: text(first.from_schema),
      toTable: text(first.to_table),
      toSchema: text(first.to_schema),
      // `string | string[]` for a composite, which is what the upstream postgres client returns. The
      // public API reference's example shows only a simple key, so its `toColumn: string` signature is
      // incomplete rather than authoritative -- protocol.ts follows the implementation.
      fromColumn: composite ? parts.map((part) => text(part.from_column)) : text(first.from_column),
      toColumn: composite ? parts.map((part) => text(part.to_column)) : text(first.to_column),
      onUpdate: text(first.on_update) || undefined,
      onDelete: text(first.on_delete) || undefined,
      direction,
    });
  }
  return keys;
};

/**
 * SQLite: foreign keys declared BY this table.
 *
 * `pragma_foreign_key_list` is a table-valued function taking the table name as text. It does accept a
 * bind parameter -- measured -- but query's QueryRequest has no parameter channel, so the name is
 * resolved against the schema first and quoted as a literal here.
 */
export const sqliteOutgoingKeysSql = (table: string): string => `
  SELECT
    p.id            AS constraint_id,
    p.seq           AS part_position,
    ${sqliteStringLiteral(table)} AS from_table,
    ''              AS from_schema,
    p."from"        AS from_column,
    p."table"       AS to_table,
    ''              AS to_schema,
    p."to"          AS to_column,
    p.on_update     AS on_update,
    p.on_delete     AS on_delete
  FROM pragma_foreign_key_list(${sqliteStringLiteral(table)}) p
  ORDER BY p.id, p.seq
`;

/**
 * SQLite: foreign keys pointing AT this table, found by asking every other table.
 *
 * SQLite has no reverse index for this -- the pragma only answers outward -- so incoming keys are a
 * scan across the schema's tables. That is why `tables` is a parameter: the caller already knows the
 * schema and this must not re-derive it.
 */
export const sqliteIncomingKeysSql = (target: string, tables: readonly string[]): string => {
  if (tables.length === 0) return 'SELECT NULL AS constraint_id WHERE 0';
  const parts = tables.map(
    (table) => `
    SELECT
      ${sqliteStringLiteral(`${table}:`)} || p.id AS constraint_id,
      p.seq        AS part_position,
      ${sqliteStringLiteral(table)} AS from_table,
      ''           AS from_schema,
      p."from"     AS from_column,
      p."table"    AS to_table,
      ''           AS to_schema,
      p."to"       AS to_column,
      p.on_update  AS on_update,
      p.on_delete  AS on_delete
    FROM pragma_foreign_key_list(${sqliteStringLiteral(table)}) p
    WHERE p."table" = ${sqliteStringLiteral(target)}`,
  );
  // The constraint id is prefixed with the owning table because pragma ids restart at 0 per table:
  // without the prefix, two tables' first foreign keys would group together as one composite.
  return `${parts.join('\n    UNION ALL\n')}\n  ORDER BY constraint_id, part_position`;
};

/** SQLite: primary key columns and their 1-based position, from pragma_table_info's `pk`. */
export const sqlitePrimaryKeysSql = (table: string): string => `
  SELECT name AS column_name, pk AS key_position
  FROM pragma_table_info(${sqliteStringLiteral(table)})
  WHERE pk > 0
  ORDER BY pk
`;

/** SQLite: indexes on a table, one row per indexed column. */
export const sqliteIndexesSql = (table: string): string => `
  SELECT
    l.name                       AS index_name,
    l.origin                     AS index_origin,
    CASE l."unique" WHEN 1 THEN 1 ELSE 0 END AS is_unique,
    i.name                       AS column_name,
    i.seqno                      AS column_position
  FROM pragma_index_list(${sqliteStringLiteral(table)}) l
  JOIN pragma_index_info(l.name) i
  ORDER BY l.seq, i.seqno
`;

export interface TableIndex {
  name: string;
  unique: boolean;
  columns: string[];
  /** SQLite's origin: `c` a CREATE INDEX, `u` a UNIQUE constraint, `pk` the primary key. */
  origin?: string;
}

export const groupIndexRows = (rows: readonly Record<string, CellValue>[]): TableIndex[] => {
  const byName = new Map<string, TableIndex>();
  for (const row of rows) {
    const name = text(row.index_name);
    const existing = byName.get(name);
    if (existing) {
      existing.columns.push(text(row.column_name));
      continue;
    }
    byName.set(name, {
      name,
      unique: Number(row.is_unique) === 1,
      origin: text(row.index_origin) || undefined,
      columns: [text(row.column_name)],
    });
  }
  return [...byName.values()];
};

export const groupPrimaryKeyRows = (rows: readonly Record<string, CellValue>[]): PrimaryKey[] =>
  rows.map((row) => ({ columnName: text(row.column_name), position: Number(row.key_position) }));

/** Refuse a dialect this module cannot read, distinguishing "none exist" from "not implemented". */
export const assertDialectSupported = (kind: DatabaseKind): void => {
  if (DIALECTS_WITHOUT_KEYS.includes(kind)) {
    throw new UnsupportedDialectError(`${kind} has no foreign keys`);
  }
  if (!KEY_READING_DIALECTS.includes(kind)) {
    throw new UnsupportedDialectError(
      `reading keys is not implemented for ${kind} yet (implemented: ${KEY_READING_DIALECTS.join(', ')})`,
    );
  }
};

/** What a read needs to know about where it is reading from. */
export interface ReadContext {
  dialect: DatabaseKind;
  /** Already resolved against metadata the database reported. */
  table: string;
  /** Required by PostgreSQL, ignored by SQLite, which has no schemas. */
  schema?: string;
  /**
   * Sibling table names, used only by SQLite's incoming-key scan.
   *
   * PostgreSQL needs none: pg_constraint records both ends of a key, so asking about the referenced
   * side is a single query. SQLite's pragma answers outward only, so incoming keys there mean asking
   * every other table.
   */
  tables?: readonly string[];
}

const pgSchema = (context: ReadContext): string => context.schema ?? 'public';

/** Read foreign keys declared BY the table. */
export const readOutgoingKeys = async (run: RunSql, context: ReadContext): Promise<TableKey[]> => {
  const sql =
    context.dialect === 'postgresql'
      ? postgresOutgoingKeysSql(context.table, pgSchema(context))
      : context.dialect === 'mysql'
        ? mysqlOutgoingKeysSql(context.table, context.schema)
        : sqliteOutgoingKeysSql(context.table);
  return groupForeignKeyRows(rowsOf(await run(sql)), 'outgoing');
};

/** Read foreign keys pointing AT the table. */
export const readIncomingKeys = async (run: RunSql, context: ReadContext): Promise<TableKey[]> => {
  const sql =
    context.dialect === 'postgresql'
      ? postgresIncomingKeysSql(context.table, pgSchema(context))
      : context.dialect === 'mysql'
        ? mysqlIncomingKeysSql(context.table, context.schema)
        : sqliteIncomingKeysSql(context.table, context.tables ?? []);
  return groupForeignKeyRows(rowsOf(await run(sql)), 'incoming');
};

export const readPrimaryKeys = async (run: RunSql, context: ReadContext): Promise<PrimaryKey[]> => {
  const sql =
    context.dialect === 'postgresql'
      ? postgresPrimaryKeysSql(context.table, pgSchema(context))
      : context.dialect === 'mysql'
        ? mysqlPrimaryKeysSql(context.table, context.schema)
        : sqlitePrimaryKeysSql(context.table);
  return groupPrimaryKeyRows(rowsOf(await run(sql)));
};

export const readIndexes = async (run: RunSql, context: ReadContext): Promise<TableIndex[]> => {
  const sql =
    context.dialect === 'postgresql'
      ? postgresIndexesSql(context.table, pgSchema(context))
      : context.dialect === 'mysql'
        ? mysqlIndexesSql(context.table, context.schema)
        : sqliteIndexesSql(context.table);
  return groupIndexRows(rowsOf(await run(sql)));
};
