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

/** Foreign key, shaped as the reference documents it. */
export interface TableKey {
  isComposite: boolean;
  fromTable: string;
  fromSchema?: string;
  fromColumn: string;
  toTable: string;
  toSchema?: string;
  toColumn: string;
  constraintName?: string;
  onUpdate?: string;
  onDelete?: string;
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
  'getViewState',
  'setViewState',
  'setTabTitle',
] as const;

/**
 * Methods the protocol defines that this host knows about and cannot yet answer.
 *
 * The three key methods need foreign key and index metadata that query's `MetadataNode` does not
 * carry -- it has id, parentId, name, kind, dataType and childCount, and nothing about constraints.
 * That data lives in the driver layer, which is where Beekeeper keeps it too, so filling these is
 * the same work as porting the dialect clients rather than a gap in this file.
 */
export const KNOWN_UNIMPLEMENTED_METHODS = [
  'getTableKeys',
  'getIncomingKeys',
  'getOutgoingKeys',
  'getPrimaryKeys',
  'getTableIndexes',
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
