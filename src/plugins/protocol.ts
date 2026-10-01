// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Beekeeper Studio plugin wire protocol, as a host must speak it.
//
// Nothing here is copied. The envelope was MEASURED from @beekeeperstudio/plugin 1.7.1 (MIT), which
// is the plugin half of this conversation, and the method semantics come from the 1,272-line public
// reference at docs/plugin_development/api-reference.md in beekeeper-studio at the commit pinned in
// docs/upstream-sources.toml. See docs/beekeeper-porting-boundary.md for why we implement the host
// rather than porting the ER diagram as a feature: the diagram is a separate GPL-3.0 plugin, and a
// host that speaks this protocol gets it along with every other plugin.

/** A plugin's request. `window.parent.postMessage({ id, name, args }, "*")`. */
export interface PluginRequest {
  id: string;
  name: string;
  args?: unknown;
}

/**
 * A host's reply to one request.
 *
 * `name` is deliberately absent, and that is a correctness requirement rather than an omission. The
 * SDK's single message listener checks `name` FIRST and dispatches notification handlers, and only
 * then checks `id` to settle the pending promise. A reply that carried both would resolve the
 * request AND fire every notification listener registered under that name. So responses carry `id`
 * and never `name`; notifications carry `name` and never `id`.
 */
export interface PluginResponse {
  id: string;
  result?: unknown;
  error?: string;
}

/** A host-initiated notification. Carries `name`, never `id`. */
export interface PluginNotification {
  name: string;
  args?: unknown;
}

/**
 * Foreign key.
 *
 * `fromColumn` and `toColumn` are `string | string[]`, and the reference documentation is what is
 * wrong here rather than this type. Its getTableKeys example shows only a single-column key with
 * `toColumn: "id"`, but the upstream postgres client returns ARRAYS for a composite key and sets
 * isComposite -- so the documented signature is incomplete, and following it would have silently
 * truncated every multi-column relationship to its first column. A simple key stays a plain string
 * rather than a one-element array, because a consumer reading a simple key should not have to handle
 * both shapes.
 */
export interface TableKey {
  isComposite: boolean;
  fromTable: string;
  fromSchema?: string;
  fromColumn: string | string[];
  toTable: string;
  toSchema?: string;
  toColumn: string | string[];
  constraintName?: string;
  onUpdate?: string;
  onDelete?: string;
  /** Which way this was read. Not in the upstream shape; useful when both sets are merged. */
  direction?: 'outgoing' | 'incoming';
}

export interface PrimaryKey {
  columnName: string;
  position: number;
}

export interface PluginTable {
  name: string;
  schema?: string;
  entityType: 'table' | 'view';
}

export interface PluginColumn {
  columnName: string;
  dataType: string;
}

export interface PluginSchema {
  name: string;
}

/**
 * Every method this host recognises, and whether it can answer it TODAY.
 *
 * The distinction is load-bearing: an unimplemented method must be refused explicitly, because the
 * failure it otherwise produces is silent and wrong. An ER diagram asking for foreign keys and
 * receiving `[]` does not report an error -- it draws every table with no relationships between
 * them, which looks like a schema that has no foreign keys rather than like a host that cannot
 * answer. A rejected promise surfaces in the plugin; an empty array lies.
 */
export const SUPPORTED_METHODS = [
  'getAppInfo',
  'getSchemas',
  'getTables',
  'getColumns',
  'getTableKeys',
  'getOutgoingKeys',
  'getIncomingKeys',
  'getPrimaryKeys',
  'getTableIndexes',
  'getViewState',
  'setViewState',
  'setTabTitle',
] as const;

/**
 * Methods the protocol defines that this host knows about and cannot yet answer.
 *
 * The five key and index methods left this list once schemaKeys.ts ported their SQL. They are answered
 * for SQLite only; the other dialects are refused BY DIALECT rather than by method, with a message
 * naming what is implemented, because "this host cannot do keys" and "this host cannot do keys on
 * MySQL" are different facts and a plugin should be told which it has hit.
 */
export const KNOWN_UNIMPLEMENTED_METHODS = [
  'runQuery',
  'getConnectionInfo',
  'expandTableResult',
  'getViewContext',
  'getData',
  'setData',
  'getEncryptedData',
  'setEncryptedData',
  'openTab',
  'openExternal',
  'requestFileSave',
  'getAppVersion',
] as const;

export type SupportedMethod = (typeof SUPPORTED_METHODS)[number];
