// SPDX-License-Identifier: GPL-3.0-or-later
//
// PostgreSQL key and index reading, ported from Beekeeper Studio's postgresql client at the commit
// pinned in docs/upstream-sources.toml, under GPL-3.0-or-later.
//
// Verified against a real PostgreSQL 18.6 server. One departure from upstream, and it is a defect fix
// rather than a preference -- see GROUPING BY OID below.

import { sqlStringLiteral } from './sqlIdentifiers';

/**
 * Both key queries share this body. Upstream repeats it twice with only the WHERE clause differing,
 * which is also where its two versions drifted apart: the outgoing one selects `a.attname AS
 * column_name` and the incoming one `a.attname AS from_column`, so the row shape differs between two
 * methods feeding the same grouping code.
 *
 * GROUPING BY OID, NOT BY CONSTRAINT NAME. Upstream groups rows with `_.groupBy(rows,
 * 'constraint_name')`. PostgreSQL scopes a constraint name to its TABLE, not its schema, so two
 * different tables may both have a foreign key called `fk_same` -- and getIncomingKeys scans every
 * table referencing the target, so those two unrelated single-column keys merge into one bogus
 * two-part composite. Measured on a live server: five rows referencing one table carried three
 * distinct connames and FOUR distinct oids, so grouping by name collapsed four real keys into three.
 * `c.oid` is one row of pg_constraint and is the actual identity.
 *
 * `generate_subscripts(c.conkey, 1)` yields a 1-based position, so the column order of a composite is
 * the `part_position` order.
 */
const keyQueryBody = `
    c.oid::text     AS constraint_id,
    c.conname       AS constraint_name,
    pos             AS part_position,
    n.nspname       AS from_schema,
    t.relname       AS from_table,
    a.attname       AS from_column,
    nf.nspname      AS to_schema,
    tf.relname      AS to_table,
    af.attname      AS to_column,
    CASE c.confupdtype::text
      WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE'
      WHEN 'n' THEN 'SET NULL'  WHEN 'd' THEN 'SET DEFAULT'
      ELSE c.confupdtype::text
    END             AS on_update,
    CASE c.confdeltype::text
      WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE'
      WHEN 'n' THEN 'SET NULL'  WHEN 'd' THEN 'SET DEFAULT'
      ELSE c.confdeltype::text
    END             AS on_delete
  FROM pg_constraint c
    JOIN pg_class t          ON c.conrelid = t.oid
    JOIN pg_namespace n      ON t.relnamespace = n.oid
    JOIN generate_subscripts(c.conkey, 1) pos ON true
    JOIN pg_attribute a      ON a.attrelid = t.oid AND a.attnum = c.conkey[pos]
    JOIN pg_class tf         ON c.confrelid = tf.oid
    JOIN pg_namespace nf     ON tf.relnamespace = nf.oid
    JOIN pg_attribute af     ON af.attrelid = tf.oid AND af.attnum = c.confkey[pos]
  WHERE c.contype = 'f'`;

const ORDER = `
  ORDER BY constraint_id, part_position`;

/** Foreign keys declared BY this table. Scoped by the REFERENCING side. */
export const postgresOutgoingKeysSql = (table: string, schema: string): string =>
  `SELECT${keyQueryBody}
    AND n.nspname = ${sqlStringLiteral(schema)}
    AND t.relname = ${sqlStringLiteral(table)}${ORDER}`;

/**
 * Foreign keys pointing AT this table. Scoped by the REFERENCED side.
 *
 * No cross-table scan is needed, unlike SQLite: pg_constraint records both ends, so asking about the
 * referenced side is one query. It also means a reference from ANOTHER schema is reported, which is
 * why `from_schema` is selected rather than assumed equal to the requested schema.
 */
export const postgresIncomingKeysSql = (table: string, schema: string): string =>
  `SELECT${keyQueryBody}
    AND nf.nspname = ${sqlStringLiteral(schema)}
    AND tf.relname = ${sqlStringLiteral(table)}${ORDER}`;

/** Primary key columns with their 1-based position within the key. */
export const postgresPrimaryKeysSql = (table: string, schema: string): string => `
  SELECT a.attname AS column_name, pos AS key_position
  FROM pg_constraint c
    JOIN pg_class t     ON c.conrelid = t.oid
    JOIN pg_namespace n ON t.relnamespace = n.oid
    JOIN generate_subscripts(c.conkey, 1) pos ON true
    JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = c.conkey[pos]
  WHERE c.contype = 'p'
    AND n.nspname = ${sqlStringLiteral(schema)}
    AND t.relname = ${sqlStringLiteral(table)}
  ORDER BY pos`;

/**
 * Indexes on a table, one row per indexed column.
 *
 * `pg_index.indkey` is an int2vector whose lower bound is ZERO, not one, so `generate_subscripts`
 * yields 0..n-1 and `indkey[pos]` is the right element. A first version subtracted one from `pos`,
 * reasoning that generate_subscripts is 1-based as it is for an ordinary array -- measured:
 * `array_lower(indkey, 1)` returns 0. The consequence was not a shifted column but a silently
 * TRUNCATED index: `indkey[-1]` is null, so the join dropped that row, which erased a single-column
 * index entirely and reduced a two-column one to its first column. Both were caught only by running
 * this against a real server with a two-column index in the fixture.
 */
export const postgresIndexesSql = (table: string, schema: string): string => `
  SELECT
    ic.relname                        AS index_name,
    CASE WHEN i.indisunique THEN 1 ELSE 0 END AS is_unique,
    CASE WHEN i.indisprimary THEN 'pk' ELSE 'c' END AS index_origin,
    a.attname                         AS column_name,
    pos                               AS column_position
  FROM pg_index i
    JOIN pg_class t      ON i.indrelid = t.oid
    JOIN pg_class ic     ON i.indexrelid = ic.oid
    JOIN pg_namespace n  ON t.relnamespace = n.oid
    JOIN generate_subscripts(i.indkey, 1) pos ON true
    JOIN pg_attribute a  ON a.attrelid = t.oid AND a.attnum = i.indkey[pos]
  WHERE n.nspname = ${sqlStringLiteral(schema)}
    AND t.relname = ${sqlStringLiteral(table)}
  ORDER BY ic.relname, pos`;
