//! Table and column creation from a form, not from free text.
//!
//! A person picks a column type from a short list; each maps to one type per dialect here. Free-form
//! type strings are not accepted -- a type is spliced into DDL (it cannot be bound), so taking one
//! from the form would be taking SQL from the form. Names are quoted as `cell_edits` quotes them.

use crate::error::{DataError, Result};
use crate::models::DatabaseKind;
use serde::{Deserialize, Serialize};

/// The column types the form offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    Text,
    Integer,
    Decimal,
    Boolean,
    Date,
    Timestamp,
}

/// One column of a new table, or a column to add.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    pub column_type: ColumnType,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default)]
    pub primary_key: bool,
}

/// A table change from the form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TableChange {
    #[serde(rename_all = "camelCase")]
    CreateTable {
        schema: Option<String>,
        table: String,
        columns: Vec<ColumnSpec>,
    },
    #[serde(rename_all = "camelCase")]
    AddColumn {
        schema: Option<String>,
        table: String,
        column: ColumnSpec,
    },
}

impl TableChange {
    /// The one statement this change runs.
    pub fn statement(&self, kind: DatabaseKind) -> Result<String> {
        match self {
            Self::CreateTable {
                schema,
                table,
                columns,
            } => create_table(kind, schema.as_deref(), table, columns),
            Self::AddColumn {
                schema,
                table,
                column,
            } => add_column(kind, schema.as_deref(), table, column),
        }
    }
}

pub const MAX_COLUMNS: usize = 200;

fn quote(kind: DatabaseKind, name: &str) -> Result<String> {
    if name.trim().is_empty() || name.contains('\0') {
        return Err(DataError::InvalidQuery(format!(
            "not a usable name: {name:?}"
        )));
    }
    Ok(match kind {
        DatabaseKind::MySql => format!("`{}`", name.replace('`', "``")),
        DatabaseKind::PostgreSql | DatabaseKind::SQLite => {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
        DatabaseKind::MongoDb | DatabaseKind::SqlServer => {
            return Err(DataError::Unsupported(
                "table changes are SQL-only and not for SQL Server in this preview".to_owned(),
            ));
        }
    })
}

fn sql_type(kind: DatabaseKind, column_type: ColumnType) -> &'static str {
    match (kind, column_type) {
        (_, ColumnType::Text) => "TEXT",
        (DatabaseKind::MySql | DatabaseKind::PostgreSql, ColumnType::Integer) => "BIGINT",
        (_, ColumnType::Decimal) => "DECIMAL(38, 10)",
        // SQLite has no boolean type; 0/1 in an INTEGER is how it stores one.
        (DatabaseKind::SQLite, ColumnType::Boolean) | (_, ColumnType::Integer) => "INTEGER",
        (_, ColumnType::Boolean) => "BOOLEAN",
        (_, ColumnType::Date) => "DATE",
        (DatabaseKind::MySql, ColumnType::Timestamp) => "DATETIME(6)",
        (_, ColumnType::Timestamp) => "TIMESTAMP",
    }
}

fn table_name(kind: DatabaseKind, schema: Option<&str>, table: &str) -> Result<String> {
    Ok(match schema {
        Some(schema) if !schema.is_empty() => {
            format!("{}.{}", quote(kind, schema)?, quote(kind, table)?)
        }
        _ => quote(kind, table)?,
    })
}

fn column_definition(kind: DatabaseKind, column: &ColumnSpec) -> Result<String> {
    let null = if column.nullable && !column.primary_key {
        ""
    } else {
        " NOT NULL"
    };
    Ok(format!(
        "{} {}{null}",
        quote(kind, &column.name)?,
        sql_type(kind, column.column_type)
    ))
}

/// `CREATE TABLE`. Refuses no columns, a name used twice, and a nullable key.
pub fn create_table(
    kind: DatabaseKind,
    schema: Option<&str>,
    table: &str,
    columns: &[ColumnSpec],
) -> Result<String> {
    if columns.is_empty() || columns.len() > MAX_COLUMNS {
        return Err(DataError::InvalidQuery(format!(
            "a new table needs 1 to {MAX_COLUMNS} columns"
        )));
    }
    let mut seen = std::collections::HashSet::new();
    let mut parts = Vec::with_capacity(columns.len() + 1);
    for column in columns {
        if !seen.insert(column.name.to_lowercase()) {
            return Err(DataError::InvalidQuery(format!(
                "column {:?} is named twice",
                column.name
            )));
        }
        parts.push(column_definition(kind, column)?);
    }
    let keys: Vec<String> = columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| quote(kind, &c.name))
        .collect::<Result<_>>()?;
    if !keys.is_empty() {
        parts.push(format!("PRIMARY KEY ({})", keys.join(", ")));
    }
    Ok(format!(
        "CREATE TABLE {} ({})",
        table_name(kind, schema, table)?,
        parts.join(", ")
    ))
}

/// `ALTER TABLE ... ADD COLUMN`. A new column must be nullable: an existing row has no value for
/// it, and a NOT NULL column without a default would refuse every row already there.
pub fn add_column(
    kind: DatabaseKind,
    schema: Option<&str>,
    table: &str,
    column: &ColumnSpec,
) -> Result<String> {
    if column.primary_key {
        return Err(DataError::InvalidQuery(
            "a key cannot be added to an existing table here".to_owned(),
        ));
    }
    if !column.nullable {
        return Err(DataError::InvalidQuery(
            "an added column must allow empty values; existing rows have none".to_owned(),
        ));
    }
    Ok(format!(
        "ALTER TABLE {} ADD COLUMN {}",
        table_name(kind, schema, table)?,
        column_definition(kind, column)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, column_type: ColumnType, nullable: bool, primary_key: bool) -> ColumnSpec {
        ColumnSpec {
            name: name.into(),
            column_type,
            nullable,
            primary_key,
        }
    }

    #[test]
    fn creates_a_table_with_mapped_types_and_a_key() {
        let columns = [
            col("id", ColumnType::Integer, false, true),
            col("name", ColumnType::Text, false, false),
            col("paid", ColumnType::Boolean, true, false),
        ];
        assert_eq!(
            create_table(DatabaseKind::PostgreSql, Some("public"), "people", &columns).unwrap(),
            r#"CREATE TABLE "public"."people" ("id" BIGINT NOT NULL, "name" TEXT NOT NULL, "paid" BOOLEAN, PRIMARY KEY ("id"))"#
        );
        assert_eq!(
            create_table(DatabaseKind::SQLite, None, "people", &columns).unwrap(),
            r#"CREATE TABLE "people" ("id" INTEGER NOT NULL, "name" TEXT NOT NULL, "paid" INTEGER, PRIMARY KEY ("id"))"#
        );
        assert_eq!(
            create_table(
                DatabaseKind::MySql,
                None,
                "we`ird",
                &[col("at", ColumnType::Timestamp, true, false)]
            )
            .unwrap(),
            "CREATE TABLE `we``ird` (`at` DATETIME(6))"
        );
    }

    #[test]
    fn adds_only_a_nullable_non_key_column() {
        assert_eq!(
            add_column(
                DatabaseKind::PostgreSql,
                None,
                "people",
                &col("email", ColumnType::Text, true, false)
            )
            .unwrap(),
            r#"ALTER TABLE "people" ADD COLUMN "email" TEXT"#
        );
        assert!(
            add_column(
                DatabaseKind::SQLite,
                None,
                "people",
                &col("email", ColumnType::Text, false, false)
            )
            .is_err()
        );
        assert!(
            add_column(
                DatabaseKind::SQLite,
                None,
                "people",
                &col("k", ColumnType::Integer, false, true)
            )
            .is_err()
        );
    }

    #[test]
    fn refuses_what_is_not_a_table() {
        assert!(create_table(DatabaseKind::SQLite, None, "t", &[]).is_err());
        assert!(
            create_table(
                DatabaseKind::SQLite,
                None,
                "t",
                &[
                    col("a", ColumnType::Text, true, false),
                    col("A", ColumnType::Text, true, false)
                ]
            )
            .is_err()
        );
        assert!(
            create_table(
                DatabaseKind::SQLite,
                None,
                " ",
                &[col("a", ColumnType::Text, true, false)]
            )
            .is_err()
        );
        assert!(
            create_table(
                DatabaseKind::SqlServer,
                None,
                "t",
                &[col("a", ColumnType::Text, true, false)]
            )
            .is_err()
        );
        assert!(
            create_table(
                DatabaseKind::MongoDb,
                None,
                "t",
                &[col("a", ColumnType::Text, true, false)]
            )
            .is_err()
        );
    }

    #[test]
    fn a_name_cannot_close_the_quote() {
        let sql = create_table(
            DatabaseKind::PostgreSql,
            None,
            "x\"; DROP TABLE y; --",
            &[col("a", ColumnType::Text, true, false)],
        )
        .unwrap();
        assert!(
            sql.starts_with(r#"CREATE TABLE "x""; DROP TABLE y; --" ("#),
            "{sql}"
        );
    }
}
