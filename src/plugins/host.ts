// SPDX-License-Identifier: GPL-3.0-or-later
//
// The host half of the Beekeeper plugin protocol: it listens for a plugin's postMessage requests and
// answers them from query's own data layer. See protocol.ts for where the wire format came from.

import type { DataBridge } from '../api/bridge';
import type { MetadataNode } from '../domain/types';
import {
  KNOWN_UNIMPLEMENTED_METHODS,
  type PluginColumn,
  type PluginNotification,
  type PluginRequest,
  type PluginResponse,
  type PluginSchema,
  type PluginTable,
  SUPPORTED_METHODS,
} from './protocol';

/** What the host needs from the application around it. */
export interface PluginHostContext {
  bridge: DataBridge;
  /** The connection a plugin's requests are answered against. */
  activeConnectionId(): string | null;
  appName: string;
  appVersion: string;
  /** Persisted per-view state, so a plugin can survive a reload. */
  readViewState(viewId: string): string | null;
  writeViewState(viewId: string, state: string): void;
  onTabTitle?(viewId: string, title: string): void;
}

const isRequest = (value: unknown): value is PluginRequest => {
  if (typeof value !== 'object' || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return typeof candidate.id === 'string' && candidate.id.length > 0 && typeof candidate.name === 'string';
};

/**
 * One mounted plugin view.
 *
 * A host instance is bound to ONE iframe, not to the window, and every arriving message is checked
 * against that iframe's own `contentWindow` before it is answered. A window-level listener that
 * answered any message would answer a second plugin's requests with the first plugin's view state,
 * and -- because plugin frames are untrusted third-party HTML -- would answer a request the page
 * itself never mounted.
 */
export class PluginHost {
  private readonly frame: HTMLIFrameElement;
  private readonly context: PluginHostContext;
  private readonly viewId: string;
  private listening = false;
  /**
   * Memoised `loadMetadata` results, keyed by connection and parent node.
   *
   * Not an optimisation added on suspicion -- it was measured. Without it every `getColumns` walks
   * the tree from the root again, and an ER diagram calls `getColumns` once per table: six tables
   * cost 28 `loadMetadata` round trips, so a fifty-table schema costs about two hundred. The cache
   * turns that into one call per node actually visited.
   *
   * Invalidation is explicit rather than time-based, because a wrong ER diagram is worse than a slow
   * one: `invalidate()` is the application's to call when the connection or the schema changes.
   */
  private readonly metadata = new Map<string, Promise<MetadataNode[]>>();
  private readonly listener = (event: MessageEvent) => {
    void this.handle(event);
  };

  constructor(frame: HTMLIFrameElement, viewId: string, context: PluginHostContext) {
    this.frame = frame;
    this.viewId = viewId;
    this.context = context;
  }

  start(): void {
    if (this.listening) return;
    this.listening = true;
    window.addEventListener('message', this.listener);
  }

  stop(): void {
    if (!this.listening) return;
    this.listening = false;
    window.removeEventListener('message', this.listener);
  }

  /** Drop cached metadata. Call when the connection changes or the schema may have been altered. */
  invalidate(): void {
    this.metadata.clear();
  }

  /** Send a notification. Carries `name` and never `id`, per protocol.ts. */
  notify(notification: PluginNotification): void {
    this.frame.contentWindow?.postMessage(notification, '*');
  }

  private async handle(event: MessageEvent): Promise<void> {
    // The identity check is the source window, not the origin. A plugin is loaded from a blob or a
    // bundled asset, so its origin is not a stable value to compare against, whereas the
    // contentWindow of the frame this host was constructed with is exactly the one party it should
    // ever answer.
    if (!this.frame.contentWindow || event.source !== this.frame.contentWindow) return;
    if (!isRequest(event.data)) return;

    const request = event.data;
    try {
      const result = await this.dispatch(request);
      this.reply({ id: request.id, result });
    } catch (error) {
      this.reply({ id: request.id, error: error instanceof Error ? error.message : String(error) });
    }
  }

  private reply(response: PluginResponse): void {
    this.frame.contentWindow?.postMessage(response, '*');
  }

  private async dispatch(request: PluginRequest): Promise<unknown> {
    const args = (request.args ?? {}) as Record<string, unknown>;

    switch (request.name) {
      case 'getAppInfo':
        return { name: this.context.appName, version: this.context.appVersion };

      case 'getSchemas':
        return this.schemas();

      case 'getTables':
        return this.tables(typeof args.schema === 'string' ? args.schema : undefined);

      case 'getColumns': {
        const table = args.table;
        if (typeof table !== 'string' || table.length === 0) {
          throw new Error('getColumns requires a table name');
        }
        return this.columns(table, typeof args.schema === 'string' ? args.schema : undefined);
      }

      case 'getViewState':
        return this.context.readViewState(this.viewId);

      case 'setViewState': {
        const state = args.state;
        if (typeof state !== 'string') throw new Error('setViewState requires a string state');
        this.context.writeViewState(this.viewId, state);
        return undefined;
      }

      case 'setTabTitle': {
        const title = args.title;
        if (typeof title !== 'string') throw new Error('setTabTitle requires a string title');
        this.context.onTabTitle?.(this.viewId, title);
        return undefined;
      }

      default:
        // Refused explicitly, never answered with an empty value. A plugin that asks for foreign
        // keys and receives [] draws a schema with no relationships and reports nothing wrong.
        if ((KNOWN_UNIMPLEMENTED_METHODS as readonly string[]).includes(request.name)) {
          throw new Error(
            `${request.name} is part of the plugin protocol but this host cannot answer it yet`,
          );
        }
        throw new Error(`unknown plugin request: ${request.name}`);
    }
  }

  private async nodes(parentId?: string | null): Promise<MetadataNode[]> {
    const connectionId = this.context.activeConnectionId();
    if (!connectionId) throw new Error('no active connection');
    const key = `${connectionId}\u0000${parentId ?? ''}`;
    const cached = this.metadata.get(key);
    if (cached) return cached;
    // The PROMISE is cached, not the resolved value, so two concurrent requests for the same node
    // share one round trip instead of racing and issuing two.
    const pending = this.context.bridge.loadMetadata(connectionId, parentId);
    this.metadata.set(key, pending);
    try {
      return await pending;
    } catch (error) {
      // A failed read must not be remembered as a result, or one transient error would poison this
      // node for the host's whole lifetime.
      this.metadata.delete(key);
      throw error;
    }
  }

  private async schemas(): Promise<PluginSchema[]> {
    // query's metadata is a tree whose root is the DATABASE, with schemas one level below, so
    // schemas are gathered per database rather than read from the root.
    const databases = await this.nodes(null);
    const collected: PluginSchema[] = [];
    for (const database of databases.filter((node) => node.kind === 'database')) {
      const children = await this.nodes(database.id);
      for (const child of children) {
        if (child.kind === 'schema') collected.push({ name: child.name });
      }
    }
    return collected;
  }

  private async schemaNodes(schema?: string): Promise<MetadataNode[]> {
    const databases = await this.nodes(null);
    const found: MetadataNode[] = [];
    for (const database of databases.filter((node) => node.kind === 'database')) {
      const children = await this.nodes(database.id);
      for (const child of children) {
        if (child.kind !== 'schema') continue;
        if (schema === undefined || child.name === schema) found.push(child);
      }
    }
    return found;
  }

  private async tables(schema?: string): Promise<PluginTable[]> {
    const schemas = await this.schemaNodes(schema);
    const tables: PluginTable[] = [];
    for (const node of schemas) {
      for (const child of await this.nodes(node.id)) {
        if (child.kind === 'table' || child.kind === 'view') {
          tables.push({ name: child.name, schema: node.name, entityType: child.kind });
        }
      }
    }
    return tables;
  }

  private async columns(table: string, schema?: string): Promise<PluginColumn[]> {
    const schemas = await this.schemaNodes(schema);
    for (const node of schemas) {
      for (const child of await this.nodes(node.id)) {
        if ((child.kind !== 'table' && child.kind !== 'view') || child.name !== table) continue;
        const columns = await this.nodes(child.id);
        return columns
          .filter((column) => column.kind === 'column')
          .map((column) => ({ columnName: column.name, dataType: column.dataType ?? 'unknown' }));
      }
    }
    throw new Error(`table not found: ${schema ? `${schema}.${table}` : table}`);
  }
}

/** Method names this host answers, for tests and for a capability report. */
export const supportedMethods = (): readonly string[] => SUPPORTED_METHODS;
