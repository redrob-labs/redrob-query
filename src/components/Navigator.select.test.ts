import { describe, expect, it } from 'vitest';
import { selectStatement } from './Navigator';

// The row menu's "Query this table" writes this into a new tab, so each dialect must get its own
// identifier quoting -- a PostgreSQL-quoted name is a syntax error on MySQL.
describe('selectStatement', () => {
  it('quotes per dialect and qualifies with the schema when known', () => {
    expect(selectStatement('postgresql', 'orders', 'sales')).toBe('SELECT *\nFROM "sales"."orders"\nLIMIT 100');
    expect(selectStatement('mysql', 'orders')).toBe('SELECT *\nFROM `orders`\nLIMIT 100');
    expect(selectStatement('sqlite', 'orders')).toBe('SELECT *\nFROM "orders"\nLIMIT 100');
    expect(selectStatement('sqlserver', 'orders', 'dbo')).toBe('SELECT *\nFROM [dbo].[orders]');
  });
  it('escapes a quote inside the name instead of ending the identifier', () => {
    expect(selectStatement('postgresql', 'we"ird')).toBe('SELECT *\nFROM "we""ird"\nLIMIT 100');
    expect(selectStatement('mysql', 'we`ird')).toBe('SELECT *\nFROM `we``ird`\nLIMIT 100');
    expect(selectStatement('sqlserver', 'we]ird')).toBe('SELECT *\nFROM [we]]ird]');
  });
});
