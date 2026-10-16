//! Key and index metadata of one table (or collection): primary key, foreign keys, indexes.
//!
//! The structure panel reads these. They used to be Beekeeper-ported TypeScript querying through
//! the Tauri bridge; here they are plain dialect queries against the live pool, with every
//! identifier passed as a bound value and resolved by the database itself where the dialect
//! allows it (`PostgreSQL`, `SQLite`'s table-valued pragmas, `information_schema` on `MySQL`).
//! `MongoDB` lists the collection's indexes; it has no keys to speak of beyond `_id`.

use futures_util::TryStreamExt as _;
use mongodb::{Client as MongoClient, bson::Document};
use serde::{Deserialize, Serialize};
use sqlx::{MySqlPool, PgPool, Row, SqlitePool};

use crate::{DataError, Result};

/// One foreign-key constraint, column by column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKey {
    /// The constraint name; empty on `SQLite`, which names none.
    pub name: String,
    /// Columns of this table, in constraint order.
    pub columns: Vec<String>,
    /// The table the columns point at, schema-qualified when the dialect has schemas.
    pub referenced_table: String,
    /// Columns of the referenced table, parallel to `columns`.
    pub referenced_columns: Vec<String>,
    #[serde(default)]
    pub on_update: String,
    #[serde(default)]
    pub on_delete: String,
}

/// One index: its columns in order, whether it is unique, and whether it is the primary key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

/// What the structure panel shows under the columns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableKeys {
    /// Primary-key columns in key order; empty when the table has none.
    pub primary: Vec<String>,
    pub foreign: Vec<ForeignKey>,
    pub indexes: Vec<IndexInfo>,
}

fn failed(error: &impl std::fmt::Display) -> DataError {
    DataError::database("key metadata lookup failed", error)
}

/// Folds `(group, member)` rows, which arrive sorted by group then position, into groups.
fn group_rows<K: PartialEq + Clone, V>(rows: Vec<(K, V)>) -> Vec<(K, Vec<V>)> {
    let mut groups: Vec<(K, Vec<V>)> = Vec::new();
    for (key, value) in rows {
        match groups.last_mut() {
            Some((last, members)) if *last == key => members.push(value),
            _ => groups.push((key, vec![value])),
        }
    }
    groups
}

pub(crate) async fn postgres_table_keys(
    pool: &PgPool,
    schema: Option<&str>,
    table: &str,
) -> Result<TableKeys> {
    let primary = sqlx::query_scalar::<_, String>(
        "SELECT a.attname::text FROM pg_index i \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
         WHERE i.indisprimary AND i.indrelid = to_regclass(CASE WHEN $1::text IS NULL THEN quote_ident($2) ELSE quote_ident($1) || '.' || quote_ident($2) END) \
         ORDER BY array_position(i.indkey, a.attnum)",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| failed(&e))?;

    // One row per (constraint, column position): the constraint's columns and the referenced
    // table's columns are parallel arrays in pg_constraint, unnested side by side.
    let fk_rows = sqlx::query(
        "SELECT c.conname::text, a.attname::text, \
                quote_ident(rn.nspname) || '.' || quote_ident(rc.relname) AS referenced_table, \
                ra.attname::text AS referenced_column, \
                c.confupdtype::text, c.confdeltype::text \
         FROM pg_constraint c \
         JOIN unnest(c.conkey, c.confkey) WITH ORDINALITY AS k(attnum, refattnum, position) ON true \
         JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum \
         JOIN pg_class rc ON rc.oid = c.confrelid \
         JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
         JOIN pg_attribute ra ON ra.attrelid = c.confrelid AND ra.attnum = k.refattnum \
         WHERE c.contype = 'f' AND c.conrelid = to_regclass(CASE WHEN $1::text IS NULL THEN quote_ident($2) ELSE quote_ident($1) || '.' || quote_ident($2) END) \
         ORDER BY c.conname, k.position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| failed(&e))?;
    let mut fk_pairs = Vec::with_capacity(fk_rows.len());
    for row in &fk_rows {
        let name: String = row.try_get(0).map_err(|e| failed(&e))?;
        let column: String = row.try_get(1).map_err(|e| failed(&e))?;
        let referenced_table: String = row.try_get(2).map_err(|e| failed(&e))?;
        let referenced_column: String = row.try_get(3).map_err(|e| failed(&e))?;
        let on_update: String = row.try_get(4).map_err(|e| failed(&e))?;
        let on_delete: String = row.try_get(5).map_err(|e| failed(&e))?;
        fk_pairs.push((
            (name, referenced_table, on_update, on_delete),
            (column, referenced_column),
        ));
    }
    let foreign = group_rows(fk_pairs)
        .into_iter()
        .map(|((name, referenced_table, on_update, on_delete), pairs)| {
            let (columns, referenced_columns) = pairs.into_iter().unzip();
            ForeignKey {
                name,
                columns,
                referenced_table,
                referenced_columns,
                on_update: postgres_action(&on_update),
                on_delete: postgres_action(&on_delete),
            }
        })
        .collect();

    let index_rows = sqlx::query(
        "SELECT ic.relname::text, a.attname::text, i.indisunique, i.indisprimary \
         FROM pg_index i \
         JOIN pg_class ic ON ic.oid = i.indexrelid \
         JOIN unnest(i.indkey) WITH ORDINALITY AS k(attnum, position) ON true \
         LEFT JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum \
         WHERE i.indrelid = to_regclass(CASE WHEN $1::text IS NULL THEN quote_ident($2) ELSE quote_ident($1) || '.' || quote_ident($2) END) \
         ORDER BY ic.relname, k.position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| failed(&e))?;
    let mut index_pairs = Vec::with_capacity(index_rows.len());
    for row in &index_rows {
        let name: String = row.try_get(0).map_err(|e| failed(&e))?;
        // An expression index has no attribute for the position; it shows as "(expression)".
        let column: Option<String> = row.try_get(1).map_err(|e| failed(&e))?;
        let unique: bool = row.try_get(2).map_err(|e| failed(&e))?;
        let is_primary: bool = row.try_get(3).map_err(|e| failed(&e))?;
        index_pairs.push((
            (name, unique, is_primary),
            column.unwrap_or_else(|| "(expression)".to_owned()),
        ));
    }
    let indexes = group_rows(index_pairs)
        .into_iter()
        .map(|((name, unique, primary), columns)| IndexInfo {
            name,
            columns,
            unique,
            primary,
        })
        .collect();

    Ok(TableKeys {
        primary,
        foreign,
        indexes,
    })
}

/// `pg_constraint.confupdtype` / `confdeltype` letters, spelled out.
fn postgres_action(code: &str) -> String {
    match code {
        "a" => "NO ACTION",
        "r" => "RESTRICT",
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        _ => "",
    }
    .to_owned()
}

pub(crate) async fn mysql_table_keys(
    pool: &MySqlPool,
    schema: Option<&str>,
    table: &str,
) -> Result<TableKeys> {
    // information_schema with the schema bound (DATABASE() when the profile names none). Column
    // lists arrive one row per position, in SEQ_IN_INDEX / ORDINAL_POSITION order.
    let schema_expr = schema.filter(|s| !s.is_empty());
    let stat_rows = sqlx::query(
        "SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = COALESCE(?, DATABASE()) AND TABLE_NAME = ? \
         ORDER BY INDEX_NAME, SEQ_IN_INDEX",
    )
    .bind(schema_expr)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| failed(&e))?;
    let mut index_pairs = Vec::with_capacity(stat_rows.len());
    for row in &stat_rows {
        let name: String = row.try_get(0).map_err(|e| failed(&e))?;
        let column: Option<String> = row.try_get(1).map_err(|e| failed(&e))?;
        let non_unique: i64 = row.try_get(2).map_err(|e| failed(&e))?;
        let primary = name == "PRIMARY";
        index_pairs.push((
            (name, non_unique == 0, primary),
            column.unwrap_or_else(|| "(expression)".to_owned()),
        ));
    }
    let indexes: Vec<IndexInfo> = group_rows(index_pairs)
        .into_iter()
        .map(|((name, unique, primary), columns)| IndexInfo {
            name,
            columns,
            unique,
            primary,
        })
        .collect();
    let primary = indexes
        .iter()
        .find(|index| index.primary)
        .map(|index| index.columns.clone())
        .unwrap_or_default();

    let fk_rows = sqlx::query(
        "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, \
                CONCAT(k.REFERENCED_TABLE_SCHEMA, '.', k.REFERENCED_TABLE_NAME), k.REFERENCED_COLUMN_NAME, \
                r.UPDATE_RULE, r.DELETE_RULE \
         FROM information_schema.KEY_COLUMN_USAGE k \
         JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
           ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
         WHERE k.TABLE_SCHEMA = COALESCE(?, DATABASE()) AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
         ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
    )
    .bind(schema_expr)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| failed(&e))?;
    let mut fk_pairs = Vec::with_capacity(fk_rows.len());
    for row in &fk_rows {
        let name: String = row.try_get(0).map_err(|e| failed(&e))?;
        let column: String = row.try_get(1).map_err(|e| failed(&e))?;
        let referenced_table: String = row.try_get(2).map_err(|e| failed(&e))?;
        let referenced_column: String = row.try_get(3).map_err(|e| failed(&e))?;
        let on_update: String = row.try_get(4).map_err(|e| failed(&e))?;
        let on_delete: String = row.try_get(5).map_err(|e| failed(&e))?;
        fk_pairs.push((
            (name, referenced_table, on_update, on_delete),
            (column, referenced_column),
        ));
    }
    let foreign = group_rows(fk_pairs)
        .into_iter()
        .map(|((name, referenced_table, on_update, on_delete), pairs)| {
            let (columns, referenced_columns) = pairs.into_iter().unzip();
            ForeignKey {
                name,
                columns,
                referenced_table,
                referenced_columns,
                on_update,
                on_delete,
            }
        })
        .collect();

    Ok(TableKeys {
        primary,
        foreign,
        indexes,
    })
}

pub(crate) async fn sqlite_table_keys(pool: &SqlitePool, table: &str) -> Result<TableKeys> {
    // Table-valued pragmas take the name as a bound value, unlike `PRAGMA x(name)`.
    let primary = sqlx::query_scalar::<_, String>(
        "SELECT name FROM pragma_table_info(?) WHERE pk > 0 ORDER BY pk",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| failed(&e))?;

    let fk_rows = sqlx::query(
        "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete FROM pragma_foreign_key_list(?) ORDER BY id, seq",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| failed(&e))?;
    let mut fk_pairs = Vec::with_capacity(fk_rows.len());
    for row in &fk_rows {
        let id: i64 = row.try_get(0).map_err(|e| failed(&e))?;
        let referenced_table: String = row.try_get(2).map_err(|e| failed(&e))?;
        let column: String = row.try_get(3).map_err(|e| failed(&e))?;
        // `to` is NULL when the constraint points at the parent's primary key implicitly.
        let referenced_column: Option<String> = row.try_get(4).map_err(|e| failed(&e))?;
        let on_update: String = row.try_get(5).map_err(|e| failed(&e))?;
        let on_delete: String = row.try_get(6).map_err(|e| failed(&e))?;
        fk_pairs.push((
            (id, referenced_table, on_update, on_delete),
            (column, referenced_column.unwrap_or_default()),
        ));
    }
    let foreign = group_rows(fk_pairs)
        .into_iter()
        .map(|((_, referenced_table, on_update, on_delete), pairs)| {
            let (columns, referenced_columns) = pairs.into_iter().unzip();
            ForeignKey {
                name: String::new(),
                columns,
                referenced_table,
                referenced_columns,
                on_update,
                on_delete,
            }
        })
        .collect();

    let index_rows =
        sqlx::query("SELECT name, \"unique\", origin FROM pragma_index_list(?) ORDER BY seq")
            .bind(table)
            .fetch_all(pool)
            .await
            .map_err(|e| failed(&e))?;
    let mut indexes = Vec::with_capacity(index_rows.len());
    for row in &index_rows {
        let name: String = row.try_get(0).map_err(|e| failed(&e))?;
        let unique: i64 = row.try_get(1).map_err(|e| failed(&e))?;
        let origin: String = row.try_get(2).map_err(|e| failed(&e))?;
        let columns = sqlx::query_scalar::<_, Option<String>>(
            "SELECT name FROM pragma_index_info(?) ORDER BY seqno",
        )
        .bind(&name)
        .fetch_all(pool)
        .await
        .map_err(|e| failed(&e))?
        .into_iter()
        .map(|column| column.unwrap_or_else(|| "(expression)".to_owned()))
        .collect();
        indexes.push(IndexInfo {
            name,
            columns,
            unique: unique != 0,
            primary: origin == "pk",
        });
    }

    Ok(TableKeys {
        primary,
        foreign,
        indexes,
    })
}

pub(crate) async fn mongo_collection_keys(
    client: &MongoClient,
    database: &str,
    collection: &str,
) -> Result<TableKeys> {
    let mut cursor = client
        .database(database)
        .collection::<Document>(collection)
        .list_indexes()
        .await
        .map_err(|e| failed(&e))?;
    let mut indexes = Vec::new();
    while let Some(model) = cursor.try_next().await.map_err(|e| failed(&e))? {
        let name = model
            .options
            .as_ref()
            .and_then(|options| options.name.clone())
            .unwrap_or_default();
        let unique = model
            .options
            .as_ref()
            .and_then(|options| options.unique)
            .unwrap_or(false);
        let columns = model.keys.keys().cloned().collect::<Vec<_>>();
        let primary = name == "_id_";
        indexes.push(IndexInfo {
            name,
            columns,
            unique: unique || primary,
            primary,
        });
    }
    Ok(TableKeys {
        primary: vec!["_id".to_owned()],
        foreign: Vec::new(),
        indexes,
    })
}
