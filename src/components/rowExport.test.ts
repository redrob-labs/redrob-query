import { describe, expect, it } from 'vitest';
import type { CellValue } from '../domain/types';
import { rowAsJson, rowAsTsv } from './rowExport';

const columns = [{ key: 'id', label: 'id' }, { key: 'note', label: 'note' }, { key: 'meta', label: 'meta' }, { key: 'gone', label: 'gone' }];
const row: Record<string, CellValue> = { id: 7, note: 'two\tparts\nand a line', meta: { plan: 'scale' }, gone: null };
const value = (key: string) => row[key];

describe('row export', () => {
  it('keeps types and null in JSON', () => {
    expect(JSON.parse(rowAsJson(columns, value))).toEqual({ id: 7, note: 'two\tparts\nand a line', meta: { plan: 'scale' }, gone: null });
  });

  it('writes one TSV row per line, escaping tabs and line breaks inside values', () => {
    const [header, body, ...rest] = rowAsTsv(columns, value).split('\n');
    expect(rest).toEqual([]);
    expect(header.split('\t')).toEqual(['id', 'note', 'meta', 'gone']);
    expect(body.split('\t')).toEqual(['7', 'two\\tparts\\nand a line', '{"plan":"scale"}', '']);
  });
});
