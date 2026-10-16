//! Staged cell edits -> UPDATE statements.
//!
//! The grid stages edits as (row primary key, column, new value). Applying them is one UPDATE per
//! row, keyed by the primary key, with every value bound as a parameter: no value is ever spliced
//! into SQL. Identifiers cannot be bound, so they are quoted for the dialect and refused when they
//! carry a NUL or are empty -- the names come from the database's own metadata, and anything else
//! is not a name this code should be writing to.
//!
//! The statements run in one transaction by the caller, and each must change exactly one row: a
//! key that matches none (the row was deleted) or several (the "key" was not unique) rolls back
//! everything, rather than half-applying a change set the person reviewed as a whole.

use crate::error::{DataError, Result};
use crate::models::{DataValue, DatabaseKind};
use serde::{Deserialize, Serialize};

/// One column's new value in one row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellChange {
    pub column: String,
    pub value: DataValue,
}

/// Every staged change to one row, found by its primary key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowEdit {
    pub key: DataValue,
    pub changes: Vec<CellChange>,
}

/// A new row: the columns the person filled in. Columns left out take the table's defaults, so an
/// auto-increment key may be omitted and the database assigns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowInsert {
    pub values: Vec<CellChange>,
}

/// A reviewed change set for one table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellEditSet {
    #[serde(default)]
    pub schema: Option<String>,
    pub table: String,
    pub primary_key: String,
    #[serde(default)]
    pub rows: Vec<RowEdit>,
    /// New rows. Applied after the updates and before the deletes, in the same transaction.
    #[serde(default)]
    pub inserts: Vec<RowInsert>,
    /// Keys of rows to delete. Applied last, in the same transaction.
    #[serde(default)]
    pub deletes: Vec<DataValue>,
}

/// One UPDATE and the values it binds, in placeholder order.
#[derive(Debug, Clone, PartialEq)]
pub struct RowUpdate {
    pub statement: String,
    pub parameters: Vec<DataValue>,
}

pub const MAX_EDITED_ROWS: usize = 1_000;

fn quote(kind: DatabaseKind, name: &str) -> Result<String> {
    if name.is_empty() || name.contains('\0') {
        return Err(DataError::InvalidQuery(format!(
            "not a usable identifier: {name:?}"
        )));
    }
    Ok(match kind {
        DatabaseKind::MySql => format!("`{}`", name.replace('`', "``")),
        DatabaseKind::PostgreSql | DatabaseKind::SQLite => {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
        DatabaseKind::MongoDb | DatabaseKind::SqlServer => {
            return Err(DataError::Unsupported(
                "cell edits are SQL-only and not for SQL Server in this preview".to_owned(),
            ));
        }
    })
}

fn placeholder(kind: DatabaseKind, index: usize) -> String {
    match kind {
        DatabaseKind::PostgreSql => format!("${index}"),
        _ => "?".to_owned(),
    }
}

/// One UPDATE per row, in the order given. Refuses an empty set, a row with no changes, a change to
/// the key column itself (the row would lose the identity that found it), and a column named twice.
pub fn build_row_updates(kind: DatabaseKind, edits: &CellEditSet) -> Result<Vec<RowUpdate>> {
    if edits.rows.is_empty() && edits.inserts.is_empty() && edits.deletes.is_empty() {
        return Err(DataError::InvalidQuery("no staged changes".to_owned()));
    }
    if edits.rows.len() + edits.inserts.len() + edits.deletes.len() > MAX_EDITED_ROWS {
        return Err(DataError::InvalidQuery(format!(
            "at most {MAX_EDITED_ROWS} rows can be applied at once"
        )));
    }
    // A row both edited and deleted is a change set nobody can have meant as a whole.
    if edits
        .deletes
        .iter()
        .any(|key| edits.rows.iter().any(|row| &row.key == key))
    {
        return Err(DataError::InvalidQuery(
            "a row is both edited and deleted".to_owned(),
        ));
    }
    let table = match &edits.schema {
        Some(schema) if !schema.is_empty() => {
            format!("{}.{}", quote(kind, schema)?, quote(kind, &edits.table)?)
        }
        _ => quote(kind, &edits.table)?,
    };
    let key = quote(kind, &edits.primary_key)?;
    let key_column = key.clone();
    edits
        .rows
        .iter()
        .map(|row| {
            if row.changes.is_empty() {
                return Err(DataError::InvalidQuery("a row has no changes".to_owned()));
            }
            let mut seen = std::collections::HashSet::new();
            let mut sets = Vec::with_capacity(row.changes.len());
            let mut parameters = Vec::with_capacity(row.changes.len() + 1);
            for (index, change) in row.changes.iter().enumerate() {
                if change.column == edits.primary_key {
                    return Err(DataError::InvalidQuery(
                        "the primary key itself cannot be edited".to_owned(),
                    ));
                }
                if !seen.insert(change.column.as_str()) {
                    return Err(DataError::InvalidQuery(format!(
                        "column {:?} is changed twice in one row",
                        change.column
                    )));
                }
                sets.push(format!(
                    "{} = {}",
                    quote(kind, &change.column)?,
                    placeholder(kind, index + 1)
                ));
                parameters.push(change.value.clone());
            }
            parameters.push(row.key.clone());
            Ok(RowUpdate {
                statement: format!(
                    "UPDATE {table} SET {} WHERE {key} = {}",
                    sets.join(", "),
                    placeholder(kind, parameters.len())
                ),
                parameters,
            })
        })
        .collect::<Result<Vec<_>>>()
        .and_then(|mut updates| {
            for insert in &edits.inserts {
                updates.push(insert_statement(kind, &table, insert)?);
            }
            updates.extend(edits.deletes.iter().map(|key| RowUpdate {
                statement: format!(
                    "DELETE FROM {table} WHERE {key_column} = {}",
                    placeholder(kind, 1)
                ),
                parameters: vec![key.clone()],
            }));
            Ok(updates)
        })
}

/// `INSERT INTO table (columns) VALUES (placeholders)`, every value bound. Refuses a row with no
/// values and a column named twice.
fn insert_statement(kind: DatabaseKind, table: &str, insert: &RowInsert) -> Result<RowUpdate> {
    if insert.values.is_empty() {
        return Err(DataError::InvalidQuery(
            "a new row has no values".to_owned(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut columns = Vec::with_capacity(insert.values.len());
    for value in &insert.values {
        if !seen.insert(value.column.as_str()) {
            return Err(DataError::InvalidQuery(format!(
                "column {:?} is set twice in a new row",
                value.column
            )));
        }
        columns.push(quote(kind, &value.column)?);
    }
    let placeholders: Vec<String> = (1..=columns.len())
        .map(|index| placeholder(kind, index))
        .collect();
    Ok(RowUpdate {
        statement: format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            columns.join(", "),
            placeholders.join(", ")
        ),
        parameters: insert
            .values
            .iter()
            .map(|value| value.value.clone())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(rows: Vec<RowEdit>) -> CellEditSet {
        CellEditSet {
            inserts: Vec::new(),
            deletes: Vec::new(),
            schema: Some("public".into()),
            table: "customers".into(),
            primary_key: "id".into(),
            rows,
        }
    }
    fn change(column: &str, value: &str) -> CellChange {
        CellChange {
            column: column.into(),
            value: DataValue::Text(value.into()),
        }
    }

    #[test]
    fn binds_every_value_and_numbers_postgres_placeholders() {
        let updates = build_row_updates(
            DatabaseKind::PostgreSql,
            &set(vec![RowEdit {
                key: DataValue::Text("cus_1".into()),
                changes: vec![change("name", "Ann"), change("plan", "Scale")],
            }]),
        )
        .unwrap();
        assert_eq!(
            updates[0].statement,
            r#"UPDATE "public"."customers" SET "name" = $1, "plan" = $2 WHERE "id" = $3"#
        );
        assert_eq!(
            updates[0].parameters,
            vec![
                DataValue::Text("Ann".into()),
                DataValue::Text("Scale".into()),
                DataValue::Text("cus_1".into())
            ]
        );
    }

    #[test]
    fn quotes_names_per_dialect_and_escapes_the_quote() {
        let edits = CellEditSet {
            inserts: Vec::new(),
            deletes: Vec::new(),
            schema: None,
            table: "we`ird\"t".into(),
            primary_key: "id".into(),
            rows: vec![RowEdit {
                key: DataValue::Integer("1".into()),
                changes: vec![change("a", "x")],
            }],
        };
        assert_eq!(
            build_row_updates(DatabaseKind::MySql, &edits).unwrap()[0].statement,
            "UPDATE `we``ird\"t` SET `a` = ? WHERE `id` = ?"
        );
        assert_eq!(
            build_row_updates(DatabaseKind::SQLite, &edits).unwrap()[0].statement,
            r#"UPDATE "we`ird""t" SET "a" = ? WHERE "id" = ?"#
        );
    }

    #[test]
    fn a_value_never_reaches_the_statement() {
        let hostile = "x'; DROP TABLE customers; --";
        let updates = build_row_updates(
            DatabaseKind::SQLite,
            &set(vec![RowEdit {
                key: DataValue::Text(hostile.into()),
                changes: vec![change("name", hostile)],
            }]),
        )
        .unwrap();
        assert!(!updates[0].statement.contains("DROP"));
    }

    #[test]
    fn inserts_bind_every_value_and_sit_between_updates_and_deletes() {
        let edits = CellEditSet {
            inserts: vec![RowInsert {
                values: vec![change("name", "Cy"), change("plan", "Free")],
            }],
            deletes: vec![DataValue::Integer("9".into())],
            ..set(vec![RowEdit {
                key: DataValue::Integer("1".into()),
                changes: vec![change("name", "Ann")],
            }])
        };
        let updates = build_row_updates(DatabaseKind::PostgreSql, &edits).unwrap();
        assert!(updates[0].statement.starts_with("UPDATE"));
        assert_eq!(
            updates[1].statement,
            r#"INSERT INTO "public"."customers" ("name", "plan") VALUES ($1, $2)"#
        );
        assert_eq!(
            updates[1].parameters,
            vec![DataValue::Text("Cy".into()), DataValue::Text("Free".into())]
        );
        assert!(updates[2].statement.starts_with("DELETE"));
        let mysql = CellEditSet {
            inserts: vec![RowInsert {
                values: vec![change("name", "x'); DROP")],
            }],
            ..set(vec![])
        };
        let insert = &build_row_updates(DatabaseKind::MySql, &mysql).unwrap()[0];
        assert_eq!(
            insert.statement,
            "INSERT INTO `public`.`customers` (`name`) VALUES (?)"
        );
        for bad in [vec![], vec![change("name", "a"), change("name", "b")]] {
            let edits = CellEditSet {
                inserts: vec![RowInsert { values: bad }],
                ..set(vec![])
            };
            assert!(build_row_updates(DatabaseKind::SQLite, &edits).is_err());
        }
    }

    #[test]
    fn deletes_follow_the_updates_and_bind_the_key() {
        let mut edits = set(vec![RowEdit {
            key: DataValue::Integer("1".into()),
            changes: vec![change("name", "Ann")],
        }]);
        edits.deletes = vec![DataValue::Integer("2".into())];
        let updates = build_row_updates(DatabaseKind::PostgreSql, &edits).unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(
            updates[1].statement,
            r#"DELETE FROM "public"."customers" WHERE "id" = $1"#
        );
        assert_eq!(updates[1].parameters, vec![DataValue::Integer("2".into())]);

        let only_deletes = CellEditSet {
            rows: vec![],
            deletes: vec![DataValue::Integer("3".into())],
            ..set(vec![])
        };
        assert_eq!(
            build_row_updates(DatabaseKind::MySql, &only_deletes).unwrap()[0].statement,
            "DELETE FROM `public`.`customers` WHERE `id` = ?"
        );

        edits.deletes = vec![DataValue::Integer("1".into())];
        assert!(
            build_row_updates(DatabaseKind::PostgreSql, &edits).is_err(),
            "edited and deleted"
        );
    }

    #[test]
    fn refuses_what_would_lose_or_blur_a_row() {
        let key = || DataValue::Integer("1".into());
        for bad in [
            set(vec![]),
            set(vec![RowEdit {
                key: key(),
                changes: vec![],
            }]),
            set(vec![RowEdit {
                key: key(),
                changes: vec![change("id", "2")],
            }]),
            set(vec![RowEdit {
                key: key(),
                changes: vec![change("name", "a"), change("name", "b")],
            }]),
            set(vec![RowEdit {
                key: key(),
                changes: vec![change("", "a")],
            }]),
        ] {
            assert!(
                build_row_updates(DatabaseKind::PostgreSql, &bad).is_err(),
                "{bad:?}"
            );
        }
        let one = set(vec![RowEdit {
            key: key(),
            changes: vec![change("name", "a")],
        }]);
        assert!(build_row_updates(DatabaseKind::SqlServer, &one).is_err());
        assert!(build_row_updates(DatabaseKind::MongoDb, &one).is_err());
    }
}
