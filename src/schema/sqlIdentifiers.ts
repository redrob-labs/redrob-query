// SPDX-License-Identifier: GPL-3.0-or-later
//
// Quoting helpers shared by the dialect modules.
//
// These exist because query's `QueryRequest` carries `query: string` and no bind parameters, so an
// identifier must be embedded in the statement text. They are defence in depth and NOT the defence:
// the host resolves every name against metadata the database itself reported before it reaches here,
// so a name the database never produced is refused by lookup rather than neutralised by escaping.

/** A single-quoted SQL string literal, doubling embedded quotes. Standard across both dialects. */
export const sqlStringLiteral = (value: string): string => `'${value.replace(/'/g, "''")}'`;

/** A double-quoted identifier, doubling embedded quotes. */
export const quoteIdentifier = (identifier: string): string => `"${identifier.replace(/"/g, '""')}"`;
