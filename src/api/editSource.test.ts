import { describe, expect, it } from 'vitest';
import { editableSource } from './editSource';

describe('editableSource', () => {
  it('finds the table of a plain select, quoted or not', () => {
    expect(editableSource('SELECT * FROM "public"."customers" LIMIT 100', 'postgresql')).toEqual({ schema: 'public', table: 'customers' });
    expect(editableSource('select id, name from people where plan = \'Free\' order by id;', 'sqlite')).toEqual({ table: 'people' });
    expect(editableSource('SELECT * FROM `shop`.`order items`', 'mysql')).toEqual({ schema: 'shop', table: 'order items' });
    expect(editableSource('SELECT * FROM "we""ird"', 'postgresql')).toEqual({ table: 'we"ird' });
  });

  it('refuses what has no single row per result row, or renames a column', () => {
    for (const sql of [
      'SELECT email AS name FROM people',
      'SELECT id, upper(name) FROM people',
      'SELECT * FROM a JOIN b ON a.id = b.id',
      'SELECT * FROM a, b',
      'SELECT DISTINCT name FROM people',
      'SELECT plan FROM people GROUP BY plan',
      'SELECT * FROM a UNION SELECT * FROM b',
      'WITH x AS (SELECT 1) SELECT * FROM x',
      'SELECT * FROM people WHERE id IN (SELECT id FROM other)',
      'SELECT * FROM people; DELETE FROM people',
      'UPDATE people SET name = 1',
    ]) {
      expect(editableSource(sql, 'postgresql'), sql).toBeNull();
    }
  });

  it('is not fooled by keywords inside string literals', () => {
    expect(editableSource("SELECT * FROM notes WHERE body = 'a join b (x)'", 'sqlite')).toEqual({ table: 'notes' });
  });

  it('refuses engines whose edits are not supported', () => {
    expect(editableSource('SELECT * FROM t', 'sqlserver')).toBeNull();
    expect(editableSource('SELECT * FROM t', 'mongodb')).toBeNull();
  });
});
