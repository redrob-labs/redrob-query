//! SQL safety classification and shared query helpers.

use crate::models::{MutationRisk, QueryClassification};

/// Classifies every statement in a SQL batch conservatively.
#[must_use]
pub fn classify_sql(sql: &str) -> QueryClassification {
    let statements = split_statements(sql);
    if statements.is_empty() {
        return QueryClassification::Unknown;
    }
    statements
        .iter()
        .map(|statement| classify_statement(statement))
        .max_by_key(|classification| severity(*classification))
        .unwrap_or(QueryClassification::Unknown)
}

/// Returns whether `sql` contains exactly one nonempty read-only statement.
#[must_use]
pub fn is_single_read_only_statement(sql: &str) -> bool {
    let statements = split_statements(sql);
    matches!(
        statements.as_slice(),
        [statement] if classify_statement(statement) == QueryClassification::ReadOnly
    )
}

#[must_use]
pub const fn risk_for(classification: QueryClassification) -> MutationRisk {
    match classification {
        QueryClassification::Mutation => MutationRisk::Elevated,
        QueryClassification::Destructive
        | QueryClassification::TransactionControl
        | QueryClassification::Unknown => MutationRisk::Destructive,
        QueryClassification::ReadOnly => MutationRisk::Low,
    }
}

fn severity(classification: QueryClassification) -> u8 {
    match classification {
        QueryClassification::ReadOnly => 0,
        QueryClassification::Mutation => 1,
        QueryClassification::TransactionControl => 2,
        QueryClassification::Destructive => 3,
        QueryClassification::Unknown => 4,
    }
}

fn classify_statement(statement: &str) -> QueryClassification {
    let tokens = tokens(statement);
    let Some(first) = tokens.first().map(String::as_str) else {
        return QueryClassification::Unknown;
    };
    if contains_mysql_file_output_clause(&tokens) {
        return QueryClassification::Destructive;
    }
    match first {
        "select" | "show" | "describe" | "desc" | "values" => QueryClassification::ReadOnly,
        "explain" => {
            if tokens.iter().any(|token| destructive_keyword(token)) {
                QueryClassification::Destructive
            } else if tokens.iter().any(|token| mutation_keyword(token)) {
                QueryClassification::Mutation
            } else {
                QueryClassification::ReadOnly
            }
        }
        "pragma" => classify_sqlite_pragma(statement),
        "with" => {
            if tokens.iter().any(|token| destructive_keyword(token)) {
                QueryClassification::Destructive
            } else if tokens.iter().any(|token| mutation_keyword(token)) {
                QueryClassification::Mutation
            } else if tokens.iter().any(|token| token == "select") {
                QueryClassification::ReadOnly
            } else {
                QueryClassification::Unknown
            }
        }
        token if mutation_keyword(token) => QueryClassification::Mutation,
        token if destructive_keyword(token) => QueryClassification::Destructive,
        "begin" | "commit" | "rollback" | "savepoint" | "release" | "start" | "set" | "use" => {
            QueryClassification::TransactionControl
        }
        _ => QueryClassification::Unknown,
    }
}

fn classify_sqlite_pragma(statement: &str) -> QueryClassification {
    let Some(pragma) = sqlite_pragma_name(statement) else {
        return QueryClassification::Unknown;
    };
    if matches!(
        pragma.as_str(),
        "collation_list"
            | "compile_options"
            | "data_version"
            | "database_list"
            | "foreign_key_check"
            | "foreign_key_list"
            | "freelist_count"
            | "function_list"
            | "index_info"
            | "index_list"
            | "index_xinfo"
            | "integrity_check"
            | "module_list"
            | "page_count"
            | "pragma_list"
            | "quick_check"
            | "table_info"
            | "table_list"
            | "table_xinfo"
    ) {
        return QueryClassification::ReadOnly;
    }
    if matches!(
        pragma.as_str(),
        "incremental_vacuum" | "optimize" | "shrink_memory" | "wal_checkpoint"
    ) || statement.contains('=')
        || statement.contains('(')
    {
        QueryClassification::Mutation
    } else {
        QueryClassification::Unknown
    }
}

fn sqlite_pragma_name(statement: &str) -> Option<String> {
    let body = statement.trim_start().get("pragma".len()..)?.trim_start();
    let header = body
        .split_once(['=', '('])
        .map_or(body, |(header, _)| header)
        .trim();
    let name = header
        .rsplit_once('.')
        .map_or(header, |(_, name)| name)
        .trim();
    (!name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_'))
    .then(|| name.to_ascii_lowercase())
}

fn contains_mysql_file_output_clause(tokens: &[String]) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0] == "into" && matches!(pair[1].as_str(), "outfile" | "dumpfile"))
}

fn mutation_keyword(token: &str) -> bool {
    matches!(token, "insert" | "update" | "upsert" | "merge" | "replace")
}

fn destructive_keyword(token: &str) -> bool {
    matches!(
        token,
        "delete"
            | "drop"
            | "truncate"
            | "alter"
            | "create"
            | "grant"
            | "revoke"
            | "vacuum"
            | "attach"
            | "detach"
    )
}

fn split_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut quote = None;
    while let Some(character) = chars.next() {
        if let Some(delimiter) = quote {
            current.push(character);
            if character == delimiter {
                if chars.peek() == Some(&delimiter) {
                    current.push(chars.next().unwrap_or(delimiter));
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match character {
            '\'' | '"' | '`' => {
                quote = Some(character);
                current.push(character);
            }
            '-' if chars.peek() == Some(&'-') => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        current.push(' ');
                        break;
                    }
                }
            }
            '#' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        current.push(' ');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let executable = chars.peek() == Some(&'!');
                if executable {
                    chars.next();
                }
                let mut body = String::new();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        body.pop();
                        break;
                    }
                    body.push(next);
                    previous = next;
                }
                current.push(' ');
                if executable {
                    current.push_str(mysql_executable_comment_body(&body));
                    current.push(' ');
                }
            }
            ';' => {
                if !current.trim().is_empty() {
                    statements.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() {
        statements.push(current);
    }
    statements
}

fn mysql_executable_comment_body(body: &str) -> &str {
    let trimmed = body.trim_start();
    let version_length = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if version_length >= 5 {
        &trimmed[version_length..]
    } else {
        body
    }
}

fn tokens(sql: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    for character in sql.chars() {
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if character.is_ascii_alphanumeric() || character == '_' {
            token.push(character.to_ascii_lowercase());
        } else if !token.is_empty() {
            result.push(std::mem::take(&mut token));
        }
    }
    if !token.is_empty() {
        result.push(token);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_read_queries_and_comments() {
        assert_eq!(
            classify_sql("-- delete everything\n SELECT * FROM users"),
            QueryClassification::ReadOnly
        );
        assert_eq!(
            classify_sql("WITH x AS (SELECT 1) SELECT * FROM x"),
            QueryClassification::ReadOnly
        );
        assert_eq!(
            classify_sql("SELECT 'drop table users'"),
            QueryClassification::ReadOnly
        );
        assert!(is_single_read_only_statement("SELECT 1;"));
        assert!(is_single_read_only_statement(
            "WITH x AS (SELECT 1) SELECT * FROM x;"
        ));
        assert!(is_single_read_only_statement(
            "SELECT ';' AS quoted /* ; */ -- ;\n;"
        ));
    }

    #[test]
    fn classifies_mutating_and_destructive_batches() {
        assert_eq!(
            classify_sql("WITH x AS (SELECT 1) UPDATE users SET active = 1"),
            QueryClassification::Mutation
        );
        assert_eq!(
            classify_sql("SELECT 1; DROP TABLE users"),
            QueryClassification::Destructive
        );
        assert_eq!(
            classify_sql("BEGIN"),
            QueryClassification::TransactionControl
        );
        assert_eq!(
            classify_sql("PRAGMA user_version(7)"),
            QueryClassification::Mutation
        );
        assert_eq!(
            classify_sql("PRAGMA table_info(customers)"),
            QueryClassification::ReadOnly
        );
        assert_eq!(classify_sql("nonsense"), QueryClassification::Unknown);
    }

    #[test]
    fn mysql_file_output_selects_are_destructive_and_not_read_only() {
        for sql in [
            "SELECT * INTO OUTFILE '/tmp/redrob.csv' FROM customers",
            "SELECT * FROM customers INTO OUTFILE '/tmp/redrob.csv'",
            "SeLeCt payload InTo DuMpFiLe '/tmp/redrob.bin' FROM blobs",
            "SELECT payload FROM blobs INTO /* server file */ DUMPFILE '/tmp/redrob.bin'",
            "SELECT payload FROM blobs INTO # server file follows\n OUTFILE '/tmp/redrob.bin'",
            "SELECT 1 /*!50000 INTO OUTFILE '/tmp/redrob.csv' */",
            "SELECT 1 /*! INTO DUMPFILE '/tmp/redrob.bin' */",
            "SELECT 1 INTO /*!50000 OUTFILE */ '/tmp/redrob.csv'",
            "SELECT 1\nInTo /* ordinary separator */ /*!50000\nDuMpFiLe */ '/tmp/redrob.bin'",
            "SELECT 1 INTO /*! 50000 OUTFILE */ '/tmp/redrob-spaced.csv'",
            "WITH selected AS (SELECT * FROM customers) SELECT * FROM selected INTO OUTFILE '/tmp/redrob.csv'",
        ] {
            assert_eq!(classify_sql(sql), QueryClassification::Destructive, "{sql}");
            assert!(!is_single_read_only_statement(sql), "{sql}");
        }

        for sql in [
            "SELECT 'INTO OUTFILE /tmp/not-a-clause' AS text",
            "SELECT 'into dumpfile' AS text",
            "SELECT 'INTO /*!50000 OUTFILE */' AS text",
            "SELECT `into`, `outfile` FROM metadata_words",
        ] {
            assert_eq!(classify_sql(sql), QueryClassification::ReadOnly, "{sql}");
            assert!(is_single_read_only_statement(sql), "{sql}");
        }
    }

    #[test]
    fn sqlite_pragmas_use_a_fail_closed_allowlist() {
        for pragma in [
            "PRAGMA incremental_vacuum",
            "PRAGMA optimize",
            "PRAGMA wal_checkpoint",
            "PRAGMA shrink_memory",
            "PRAGMA user_version(7)",
            "PRAGMA journal_mode = WAL",
        ] {
            assert_eq!(
                classify_sql(pragma),
                QueryClassification::Mutation,
                "{pragma}"
            );
        }
        assert_eq!(
            classify_sql("PRAGMA unrecognized_pragma"),
            QueryClassification::Unknown
        );
        for pragma in [
            "PRAGMA table_info(customers)",
            "PRAGMA main.table_info(customers)",
            "PRAGMA temp.index_list('customers')",
            "PRAGMA main.foreign_key_list(customers)",
            "PRAGMA database_list",
            "PRAGMA main.table_list",
        ] {
            assert_eq!(
                classify_sql(pragma),
                QueryClassification::ReadOnly,
                "{pragma}"
            );
            assert!(is_single_read_only_statement(pragma), "{pragma}");
        }
    }

    #[test]
    fn single_read_only_statement_rejects_batches_and_backslash_quote_bypass() {
        assert!(!is_single_read_only_statement(""));
        assert!(!is_single_read_only_statement("; -- empty"));
        assert!(!is_single_read_only_statement("SELECT 1; SELECT 2"));
        assert!(!is_single_read_only_statement(
            r"SELECT '\'; DELETE FROM customers; -- '"
        ));
        assert_eq!(
            classify_sql(r"SELECT '\'; DELETE FROM customers; -- '"),
            QueryClassification::Destructive
        );
    }
}
