// SPDX-License-Identifier: GPL-3.0-or-later
//
// MySQL / MariaDB key and index reading, ported from Beekeeper Studio's mysql client at the commit
// pinned in docs/upstream-sources.toml, under GPL-3.0-or-later.
//
// Verified against a real MariaDB 11.8.6 server. See docs/dialect-verification.md for how to reproduce
// that, and note what it is NOT: MariaDB, not MySQL proper. The queries use only
// information_schema.key_column_usage, referential_constraints and statistics, which both implement to
// the same shape, but the distinction is recorded rather than glossed -- upstream's own client covers
// both under one name and this port inherits that assumption.

import { sqlStringLiteral } from './sqlIdentifiers';

/**
 * THE DEFECT THIS PORT FIXES, and it is not subtle.
 *
 * Upstream's getIncomingKeys joins `information_schema.referential_constraints` with NO ON CLAUSE --
 * its getOutgoingKeys has one and the incoming copy simply lost it. That is a cartesian product:
 * measured on a live server, asking which keys reference one table returned SIXTEEN rows where FOUR
 * are correct, each carrying on_update and on_delete values pulled from an unrelated constraint.
 *
 * So the join condition is stated once here and used by both directions, which is also why the two
 * queries share a body rather than being copied -- copying is how the original lost it.
 */
const KEY_JOIN = `
  FROM information_schema.key_column_usage cu
    JOIN information_schema.referential_constraints rc
      ON  cu.constraint_name   = rc.constraint_name
      AND cu.constraint_schema = rc.constraint_schema`;

/**
 * Grouping by constraint NAME is correct here, unlike in PostgreSQL.
 *
 * The postgres port had to group by oid because PostgreSQL scopes a constraint name to its table, so
 * two tables could both have `fk_same` and upstream's groupBy(constraint_name) merged them. InnoDB
 * scopes a foreign key name to the DATABASE: creating the same pair is refused outright with errno 121,
 * measured. So the name is a sufficient identity, and the same upstream pattern that is a bug in one
 * dialect is sound in the other. `constraint_schema` is included in the id anyway, since these queries
 * are scoped to one database only by their WHERE clause.
 */
const CONSTRAINT_ID = `CONCAT(cu.constraint_schema, '.', cu.constraint_name)`;

const selectList = `
    ${CONSTRAINT_ID}          AS constraint_id,
    cu.constraint_name        AS constraint_name,
    cu.ordinal_position       AS part_position,
    cu.constraint_schema      AS from_schema,
    cu.table_name             AS from_table,
    cu.column_name            AS from_column,
    cu.referenced_table_schema AS to_schema,
    cu.referenced_table_name  AS to_table,
    cu.referenced_column_name AS to_column,
    rc.update_rule            AS on_update,
    rc.delete_rule            AS on_delete`;

const ORDER = `
  ORDER BY constraint_id, cu.ordinal_position`;

/**
 * Foreign keys declared BY this table.
 *
 * `cu.referenced_table_name IS NOT NULL` is what separates a foreign key from a primary or unique
 * constraint: key_column_usage holds all of them, and without this filter every primary key column
 * would be reported as a relationship with a null target.
 */
export const mysqlOutgoingKeysSql = (table: string, schema?: string): string => `
  SELECT${selectList}${KEY_JOIN}
  WHERE cu.constraint_schema = ${schema ? sqlStringLiteral(schema) : 'database()'}
    AND cu.table_name = ${sqlStringLiteral(table)}
    AND cu.referenced_table_name IS NOT NULL${ORDER}`;

/** Foreign keys pointing AT this table. */
export const mysqlIncomingKeysSql = (table: string, schema?: string): string => `
  SELECT${selectList}${KEY_JOIN}
  WHERE cu.constraint_schema = ${schema ? sqlStringLiteral(schema) : 'database()'}
    AND cu.referenced_table_name = ${sqlStringLiteral(table)}${ORDER}`;

/**
 * Primary key columns with their 1-based position.
 *
 * Read from `statistics` rather than key_column_usage, because MariaDB reports a primary key's
 * ordinal there as `seq_in_index` and the two agree; either would do, and statistics is the same table
 * the index query below uses, which keeps one source for column ordering.
 */
export const mysqlPrimaryKeysSql = (table: string, schema?: string): string => `
  SELECT column_name AS column_name, seq_in_index AS key_position
  FROM information_schema.statistics
  WHERE table_schema = ${schema ? sqlStringLiteral(schema) : 'database()'}
    AND table_name = ${sqlStringLiteral(table)}
    AND index_name = 'PRIMARY'
  ORDER BY seq_in_index`;

/**
 * Indexes on a table, one row per indexed column.
 *
 * `non_unique` is inverted to `is_unique` here rather than at the consumer, so both dialects hand the
 * grouping function the same column. MariaDB names the primary key index `PRIMARY`, which is reported
 * as origin `pk` to match what PostgreSQL's `indisprimary` produces.
 */
export const mysqlIndexesSql = (table: string, schema?: string): string => `
  SELECT
    index_name                                     AS index_name,
    CASE WHEN non_unique = 0 THEN 1 ELSE 0 END     AS is_unique,
    CASE WHEN index_name = 'PRIMARY' THEN 'pk' ELSE 'c' END AS index_origin,
    column_name                                    AS column_name,
    seq_in_index - 1                               AS column_position
  FROM information_schema.statistics
  WHERE table_schema = ${schema ? sqlStringLiteral(schema) : 'database()'}
    AND table_name = ${sqlStringLiteral(table)}
  ORDER BY index_name, seq_in_index`;
