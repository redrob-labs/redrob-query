import type { CellValue } from '../domain/types';

export interface ExportColumn { key: string; label: string }

/** The row as one JSON object keyed by column label, values kept as their own types (null stays null). */
export function rowAsJson(columns: ExportColumn[], value: (key: string) => CellValue): string {
  return JSON.stringify(Object.fromEntries(columns.map((column) => [column.label, value(column.key)])), null, 2);
}

// One TSV field: a tab or line break inside a value would split the row, so those are written as
// their escapes; null is empty, as spreadsheets read it; objects are their JSON.
function tsvField(cell: CellValue): string {
  if (cell === null) return '';
  const text = typeof cell === 'object' ? JSON.stringify(cell) : String(cell);
  return text.replace(/\\/g, '\\\\').replace(/\t/g, '\\t').replace(/\r?\n/g, '\\n');
}

/** A header line and the row, tab-separated, ready to paste into a spreadsheet. */
export function rowAsTsv(columns: ExportColumn[], value: (key: string) => CellValue): string {
  return [columns.map((column) => tsvField(column.label)).join('\t'), columns.map((column) => tsvField(value(column.key))).join('\t')].join('\n');
}
