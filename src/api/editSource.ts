import type { DatabaseKind } from '../domain/types';

// Which table a result can be written back to, or null. Strict on purpose: a cell edit becomes
// `UPDATE <table> SET <column> = ? WHERE <key> = ?`, so the result's columns must BE the table's
// columns. `SELECT email AS name FROM t` would put an edit of "name" into the wrong column, and a
// join, group, union or expression has no single row to write to. Anything this does not
// recognise stays read-only, which is what desktop results were before.

export interface EditableSource { schema?: string; table: string }

const IDENT = String.raw`(?:"(?:[^"]|"")+"|\x60(?:[^\x60]|\x60\x60)+\x60|\[[^\]]+\]|[A-Za-z_][A-Za-z0-9_$]*)`;
const COLUMN_LIST = String.raw`\*|${IDENT}(?:\s*,\s*${IDENT})*`;
const SOURCE = new RegExp(String.raw`^\s*select\s+(${COLUMN_LIST})\s+from\s+(${IDENT})(?:\s*\.\s*(${IDENT}))?(?:\s+(?:where|order\s+by|limit|offset)\b[\s\S]*)?\s*;?\s*$`, 'i');
const FORBIDDEN = /\b(join|union|intersect|except|group\s+by|having|distinct|with|window|over|returning|into)\b|\(/i;

function unquote(name: string): string {
  if (name.startsWith('"')) return name.slice(1, -1).replace(/""/g, '"');
  if (name.startsWith('`')) return name.slice(1, -1).replace(/``/g, '`');
  if (name.startsWith('[')) return name.slice(1, -1);
  return name;
}

// String literals can hold anything; blank them before looking for forbidden words.
const withoutStrings = (sql: string) => sql.replace(/'(?:[^']|'')*'/g, "''");

export function editableSource(sql: string, kind: DatabaseKind): EditableSource | null {
  if (kind === 'mongodb' || kind === 'sqlserver') return null;
  const text = withoutStrings(sql);
  if (text.split(';').filter((part) => part.trim()).length !== 1) return null;
  const match = SOURCE.exec(text);
  if (!match) return null;
  const [, , first, second] = match;
  // Anything after the table must not reintroduce what the shape forbids (a subquery in WHERE).
  const tail = text.slice(text.toLowerCase().indexOf(' from ') + 6);
  if (FORBIDDEN.test(tail) || FORBIDDEN.test(match[1])) return null;
  return second ? { schema: unquote(first), table: unquote(second) } : { table: unquote(first) };
}
