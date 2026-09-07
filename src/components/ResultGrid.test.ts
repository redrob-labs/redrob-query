import { describe, expect, it } from 'vitest';
import { formatCell, parseCellInput } from './ResultGrid';

describe('typed result cell parsing', () => {
  it('parses supported typed values without silent coercion', () => {
    expect(parseCellInput('1,240.5', 'number')).toBe(1240.5);
    expect(parseCellInput('false', 'boolean')).toBe(false);
    expect(parseCellInput('{"tier":"scale"}', 'json')).toEqual({ tier: 'scale' });
    expect(parseCellInput('NULL', 'string', true)).toBeNull();
  });

  it('renders BIT-compatible numeric values exactly without boolean truthiness', () => {
    expect([0, 1, 2, 5].map((value) => formatCell(value, 'number'))).toEqual([
      '0',
      '1',
      '2',
      '5',
    ]);
    expect(formatCell(false, 'boolean')).toBe('false');
    expect(formatCell(true, 'boolean')).toBe('true');
    expect(formatCell(5, 'boolean')).toBe('5');
  });

  it('rejects invalid numbers, booleans, and JSON', () => {
    expect(() => parseCellInput('not-a-number', 'number')).toThrow('valid number');
    expect(() => parseCellInput('yes', 'boolean')).toThrow('true or false');
    expect(() => parseCellInput('{oops}', 'json')).toThrow('valid JSON');
  });
});
