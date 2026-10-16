//! Syntax colouring and a small formatter for SQL and Mongo JSON (MQL).
//!
//! A hand-rolled lexer rather than syntect: the editor needs keywords, strings, numbers, comments
//! and identifiers told apart in a few hundred lines, cached per frame by egui's layouter. The
//! formatter is deliberately simple -- it puts the major clauses on their own lines and indents
//! what follows -- so it never changes the meaning of a statement it does not understand.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Ui};
use redrob_query_core::{DatabaseKind, QueryLanguage};

/// The query language the editor colours with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Sql,
    MongoJson,
}

impl Dialect {
    pub fn for_kind(kind: DatabaseKind) -> Self {
        match kind {
            DatabaseKind::MongoDb => Self::MongoJson,
            _ => Self::Sql,
        }
    }

    pub fn language(self) -> QueryLanguage {
        match self {
            Self::Sql => QueryLanguage::Sql,
            Self::MongoJson => QueryLanguage::MongoJson,
        }
    }
}

const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "IS", "NULL", "AS", "ON", "JOIN", "LEFT",
    "RIGHT", "INNER", "OUTER", "FULL", "CROSS", "GROUP", "BY", "ORDER", "HAVING", "LIMIT", "OFFSET",
    "ASC", "DESC", "DISTINCT", "UNION", "ALL", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE",
    "CREATE", "TABLE", "INDEX", "VIEW", "DROP", "ALTER", "ADD", "COLUMN", "PRIMARY", "KEY", "FOREIGN",
    "REFERENCES", "UNIQUE", "DEFAULT", "CHECK", "CONSTRAINT", "BEGIN", "COMMIT", "ROLLBACK",
    "TRANSACTION", "WITH", "RECURSIVE", "CASE", "WHEN", "THEN", "ELSE", "END", "EXISTS", "BETWEEN",
    "LIKE", "ILIKE", "CAST", "COUNT", "SUM", "AVG", "MIN", "MAX", "COALESCE", "NULLIF", "TRUE",
    "FALSE", "EXPLAIN", "ANALYZE", "SHOW", "DESCRIBE", "PRAGMA", "RETURNING", "USING", "NATURAL",
    "OVER", "PARTITION", "WINDOW", "ROWS", "RANGE", "FETCH", "FIRST", "NEXT", "ONLY", "TOP",
    "INTERSECT", "EXCEPT", "TRUNCATE", "GRANT", "REVOKE", "IF", "REPLACE", "TEMPORARY", "TEMP",
];

/// Clause-leading keywords the formatter starts a new line at (GROUP/ORDER carry their BY,
/// INSERT its INTO, UNION its ALL, and a LEFT/RIGHT/INNER/FULL/CROSS its JOIN).
const SQL_CLAUSES: &[&str] = &[
    "SELECT", "FROM", "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "UNION", "VALUES",
    "SET", "RETURNING", "INSERT", "UPDATE", "DELETE", "ON", "WITH", "INTERSECT", "EXCEPT",
];

const MQL_KEYWORDS: &[&str] = &[
    "collection", "operation", "filter", "projection", "sort", "limit", "explain", "pipeline", "find",
    "aggregate", "true", "false", "null",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    Keyword,
    Identifier,
    Number,
    Str,
    Comment,
    Operator,
    Punct,
    Mongo,
    Whitespace,
}

/// The colours of each token class, read from the theme so both themes stay legible.
struct Palette {
    keyword: Color32,
    identifier: Color32,
    number: Color32,
    string: Color32,
    comment: Color32,
    operator: Color32,
    mongo: Color32,
}

impl Palette {
    fn from_ui(ui: &Ui) -> Self {
        let dark = ui.visuals().dark_mode;
        let text = ui.visuals().text_color();
        if dark {
            Self {
                keyword: Color32::from_rgb(0x7F, 0xA6, 0xFF),
                identifier: text,
                number: Color32::from_rgb(0xD6, 0xB5, 0x6E),
                string: Color32::from_rgb(0x8F, 0xD1, 0x9A),
                comment: Color32::from_rgb(0x80, 0x86, 0x90),
                operator: Color32::from_rgb(0xC9, 0xCE, 0xD6),
                mongo: Color32::from_rgb(0xC9, 0x9A, 0xF0),
            }
        } else {
            Self {
                keyword: crate::theme::REDROB_BLUE,
                identifier: text,
                number: Color32::from_rgb(0x9A, 0x5B, 0x00),
                string: Color32::from_rgb(0x1E, 0x7A, 0x3E),
                comment: Color32::from_rgb(0x74, 0x7A, 0x84),
                operator: Color32::from_rgb(0x40, 0x46, 0x50),
                mongo: Color32::from_rgb(0x7A, 0x3E, 0xB8),
            }
        }
    }

    fn colour(&self, token: Token) -> Color32 {
        match token {
            Token::Keyword => self.keyword,
            Token::Identifier | Token::Whitespace | Token::Punct => self.identifier,
            Token::Number => self.number,
            Token::Str => self.string,
            Token::Comment => self.comment,
            Token::Operator => self.operator,
            Token::Mongo => self.mongo,
        }
    }
}

/// Splits `text` into `(token, byte range)` pieces. Every byte of the input is covered.
fn lex(text: &str, dialect: Dialect) -> Vec<(Token, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let c = bytes[i];
        let token = if c.is_ascii_whitespace() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            Token::Whitespace
        } else if dialect == Dialect::Sql && c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            Token::Comment
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            Token::Comment
        } else if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            Token::Comment
        } else if c == b'\'' || c == b'"' || c == b'`' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && dialect == Dialect::MongoJson {
                    i += 2;
                    continue;
                }
                if bytes[i] == c {
                    // SQL doubles a quote to escape it.
                    if dialect == Dialect::Sql && bytes.get(i + 1) == Some(&c) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            i = i.min(bytes.len());
            if c == b'\'' || dialect == Dialect::MongoJson {
                Token::Str
            } else {
                Token::Identifier
            }
        } else if c.is_ascii_digit() || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
            {
                i += 1;
            }
            Token::Number
        } else if c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80 {
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$' || bytes[i] >= 0x80)
            {
                i += 1;
            }
            let word = &text[start..i];
            match dialect {
                Dialect::Sql if SQL_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word)) => {
                    Token::Keyword
                }
                Dialect::MongoJson if word.starts_with('$') => Token::Mongo,
                Dialect::MongoJson if MQL_KEYWORDS.contains(&word) => Token::Keyword,
                _ => Token::Identifier,
            }
        } else if b"=<>!+-*/%|&^~".contains(&c) {
            i += 1;
            Token::Operator
        } else {
            i += 1;
            Token::Punct
        };
        out.push((token, start..i));
    }
    out
}

/// A coloured layout of `text` for the editor's layouter; `wrap_width` is the editor's width.
pub fn layout(ui: &Ui, text: &str, dialect: Dialect, wrap_width: f32) -> LayoutJob {
    let palette = Palette::from_ui(ui);
    let font = FontId::monospace(13.0);
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    for (token, range) in lex(text, dialect) {
        let mut format = TextFormat::simple(font.clone(), palette.colour(token));
        if token == Token::Comment {
            format.italics = true;
        }
        job.append(&text[range], 0.0, format);
    }
    job
}

/// Reformats `text`: SQL gets its major clauses on their own lines, keywords upper-cased outside
/// strings and comments; Mongo JSON is pretty-printed when it parses, left alone otherwise.
pub fn format(text: &str, dialect: Dialect) -> String {
    match dialect {
        Dialect::MongoJson => serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .unwrap_or_else(|| text.to_owned()),
        Dialect::Sql => format_sql(text),
    }
}

fn format_sql(text: &str) -> String {
    // Keywords upper-cased; whitespace outside strings and comments collapsed; each major clause
    // at paren depth 0 starts a new line. Strings, identifiers and comments are left untouched.
    let mut out = String::new();
    let mut depth: i32 = 0;
    let mut pending_space = false;
    let tokens: Vec<(Token, &str)> = lex(text, Dialect::Sql)
        .into_iter()
        .filter(|(token, _)| *token != Token::Whitespace)
        .map(|(token, range)| (token, &text[range]))
        .collect();
    for (index, (token, piece)) in tokens.iter().enumerate() {
        let upper = piece.to_ascii_uppercase();
        let word = if *token == Token::Keyword { upper.as_str() } else { piece };
        let previous = index.checked_sub(1).map(|i| tokens[i].1.to_ascii_uppercase());
        let starts_clause = *token == Token::Keyword
            && depth == 0
            && index > 0
            && (SQL_CLAUSES.contains(&word)
                || (word == "JOIN"
                    && !previous.as_deref().is_some_and(|p| {
                        matches!(p, "LEFT" | "RIGHT" | "INNER" | "FULL" | "CROSS" | "OUTER" | "NATURAL")
                    }))
                || (matches!(word, "LEFT" | "RIGHT" | "INNER" | "FULL" | "CROSS")
                    && tokens.get(index + 1).is_some_and(|(_, n)| {
                        let n = n.to_ascii_uppercase();
                        n == "JOIN" || n == "OUTER"
                    })));
        let is_second_word = matches!(word, "BY" | "INTO" | "ALL" | "OUTER" | "JOIN")
            && previous.as_deref().is_some_and(|p| {
                matches!(
                    p,
                    "GROUP" | "ORDER" | "INSERT" | "UNION" | "LEFT" | "RIGHT" | "INNER" | "FULL" | "CROSS" | "OUTER"
                )
            });
        if starts_clause && !is_second_word && !out.is_empty() {
            out.push('\n');
            pending_space = false;
        } else if *token == Token::Comment {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            pending_space = false;
        }
        let tight_before = matches!(*piece, "," | ")" | ";" | ".")
            || (*piece == "(" && previous.as_deref().is_some_and(|p| {
                // A call: `count(` not `IN (`.
                p.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !SQL_KEYWORDS.contains(&p.as_str())
            }));
        let tight_after = out.ends_with('(') || out.ends_with('.');
        if pending_space && !tight_before && !tight_after {
            out.push(' ');
        }
        out.push_str(word);
        if *token == Token::Comment {
            out.push('\n');
            pending_space = false;
        } else {
            pending_space = true;
        }
        match *piece {
            "(" => depth += 1,
            ")" => depth = (depth - 1).max(0),
            _ => {}
        }
    }
    out.trim_end().to_owned()
}

/// The query a new tab opens with for a connection kind, so the first run shows something real.
pub fn starter_query(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::PostgreSql => {
            "SELECT table_schema, table_name\nFROM information_schema.tables\nWHERE table_schema NOT IN ('pg_catalog', 'information_schema')\nORDER BY table_schema, table_name\nLIMIT 100;"
        }
        DatabaseKind::MySql => {
            "SELECT table_name, table_rows\nFROM information_schema.tables\nWHERE table_schema = DATABASE()\nORDER BY table_name\nLIMIT 100;"
        }
        DatabaseKind::SQLite => {
            "SELECT name, type\nFROM sqlite_master\nWHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'\nORDER BY name;"
        }
        DatabaseKind::MongoDb => {
            "{\n  \"collection\": \"\",\n  \"operation\": \"find\",\n  \"filter\": {},\n  \"limit\": 50\n}"
        }
        DatabaseKind::SqlServer => "-- SQL Server is not supported in this release.",
    }
}

/// The statement under the cursor: the text between the semicolons (or blank lines) around
/// `cursor_char`, or the whole text when there is no separator. Mongo JSON is always the whole text.
pub fn statement_at(text: &str, cursor_char: usize, dialect: Dialect) -> String {
    if dialect == Dialect::MongoJson {
        return text.trim().to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor_char.min(chars.len());
    // Walk back to the previous ';' (outside strings would be nicer; a cursor-local heuristic).
    let mut start = cursor;
    while start > 0 && chars[start - 1] != ';' {
        start -= 1;
    }
    let mut end = cursor;
    while end < chars.len() && chars[end] != ';' {
        end += 1;
    }
    let statement: String = chars[start..end].iter().collect();
    let statement = statement.trim();
    if statement.is_empty() {
        // The cursor sits right after a ';': take the statement before it.
        let before: String = chars[..start.saturating_sub(1)].iter().collect();
        return before
            .rsplit(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_owned();
    }
    statement.to_owned()
}
