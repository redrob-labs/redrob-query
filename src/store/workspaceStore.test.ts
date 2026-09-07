import { describe, expect, it, vi } from 'vitest';
import { DemoBridge, type DataBridge } from '../api/bridge';
import type { ConnectionProfile, MetadataNode, QueryResult } from '../domain/types';
import { createWorkspaceStore } from './workspaceStore';

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
};

const postgres: ConnectionProfile = {
  id: 'pg', name: 'Postgres', kind: 'postgresql', host: 'localhost', port: 5432, database: 'app', username: 'reader', tls: true, state: 'connected',
};
const mongo: ConnectionProfile = {
  id: 'mongo', name: 'Mongo', kind: 'mongodb', host: 'localhost', port: 27017, database: 'documents', username: 'reader', tls: true, authSource: 'admin', state: 'connected',
};
const pgRoot: MetadataNode[] = [{ id: 'pg-db', parentId: null, name: 'app', kind: 'database' }];
const mongoRoot: MetadataNode[] = [{ id: 'mongo-db', parentId: null, name: 'documents', kind: 'database' }];
const result = (engine: string): QueryResult => ({
  columns: [{ key: 'engine', label: 'engine', dataType: 'string' }], rows: [{ engine }], rowCount: 1, durationMs: 1,
});

const controllableBridge = (mode: DataBridge['mode'] = 'demo') => {
  const bridge: DataBridge = {
    mode,
    listConnections: vi.fn(async () => [postgres, mongo]),
    saveConnection: vi.fn(),
    testConnection: vi.fn(),
    loadMetadata: vi.fn(async (connectionId: string) => connectionId === postgres.id ? pgRoot : mongoRoot),
    executeQuery: vi.fn(async (request) => result(request.connectionId)),
    applyMutations: vi.fn(),
    saveAiKey: vi.fn(),
    askAi: vi.fn(),
  };
  return bridge;
};

describe('workspace state flows', () => {
  it('lists desktop connections without selecting one or loading metadata', async () => {
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);

    await store.getState().initialize();

    expect(bridge.listConnections).toHaveBeenCalledOnce();
    expect(bridge.loadMetadata).not.toHaveBeenCalled();
    expect(store.getState()).toEqual(expect.objectContaining({
      initialized: true,
      connections: [postgres, mongo],
      activeConnectionId: null,
      tabs: [],
      activeTabId: '',
      metadata: {},
    }));
  });

  it('loads metadata and a starter tab when the first desktop connection is explicitly selected', async () => {
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();

    await store.getState().setActiveConnection(postgres.id);

    const state = store.getState();
    const activeTab = state.tabs.find((tab) => tab.id === state.activeTabId);
    expect(state.activeConnectionId).toBe(postgres.id);
    expect(activeTab).toEqual(expect.objectContaining({
      id: 'query-1',
      connectionId: postgres.id,
      language: 'sql',
      query: expect.stringContaining('FROM public.customers'),
    }));
    expect(state.metadata.root).toEqual(pgRoot);
    expect(bridge.loadMetadata).toHaveBeenCalledOnce();
    expect(bridge.loadMetadata).toHaveBeenCalledWith(postgres.id, null);
  });

  it('keeps the browser demo ready with its seeded connection selected', async () => {
    const bridge = controllableBridge('demo');
    const store = createWorkspaceStore(bridge);

    await store.getState().initialize();

    expect(store.getState().activeConnectionId).toBe(postgres.id);
    expect(store.getState().activeTabId).toBe('query-1');
    expect(store.getState().metadata.root).toEqual(pgRoot);
    expect(bridge.loadMetadata).toHaveBeenCalledWith(postgres.id, null);
  });

  it('stages and applies an editable typed result cell', async () => {
    const store = createWorkspaceStore(new DemoBridge());
    await store.getState().initialize();
    await store.getState().runQuery();
    store.getState().stageCell(0, 'plan', 'Enterprise');
    expect(store.getState().mutations).toEqual([expect.objectContaining({ table: 'public.customers', primaryKey: 'id', rowKey: 'cus_0001', column: 'plan', nextValue: 'Enterprise' })]);
    await store.getState().applyMutations();
    expect(store.getState().mutations).toHaveLength(0);
    expect(store.getState().result?.rows[0].plan).toBe('Enterprise');
    expect(store.getState().toasts.at(-1)).toEqual(expect.objectContaining({ tone: 'success', title: 'Changes applied' }));
  });

  it('switches to a Mongo-owned starter tab, clears staged changes, and loads Mongo metadata', async () => {
    const bridge = new DemoBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().runQuery();
    store.getState().stageCell(0, 'plan', 'Enterprise');
    const profile = await bridge.saveConnection({ name: 'Documents', kind: 'mongodb', host: 'localhost', port: 27017, database: 'accounts', username: 'reader', tls: true, authSource: 'admin' });
    await store.getState().addConnection(profile);

    const state = store.getState();
    const activeTab = state.tabs.find((tab) => tab.id === state.activeTabId);
    expect(state.activeConnectionId).toBe(profile.id);
    expect(state.mutations).toHaveLength(0);
    expect(state.changesOpen).toBe(false);
    expect(state.result).toBeNull();
    expect(activeTab).toEqual(expect.objectContaining({ connectionId: profile.id, language: 'mql' }));
    expect(activeTab?.query).toContain('"operation": "find"');
    expect(state.metadata.root).toEqual([expect.objectContaining({ name: 'accounts' })]);

    await store.getState().runQuery();
    expect(store.getState().result?.rows[0]).toEqual(expect.objectContaining({ _id: expect.any(String) }));
  });

  it('uses a valid SQLite starter rather than PostgreSQL-only columns', async () => {
    const bridge = new DemoBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    const profile = await bridge.saveConnection({ name: 'Local', kind: 'sqlite', host: '', port: 0, database: '', username: '', filePath: '/tmp/local.sqlite', tls: true });
    await store.getState().addConnection(profile);
    const tab = store.getState().tabs.find((item) => item.id === store.getState().activeTabId)!;
    expect(tab.query).toContain('city');
    expect(tab.query).toContain('FROM customers');
    expect(tab.query).not.toContain('public.customers');
    expect(tab.query).not.toContain('mrr');
    await store.getState().runQuery();
    expect(store.getState().result?.rows[0]).toEqual(expect.objectContaining({ city: 'Austin' }));
  });

  it('ignores stale metadata and query completions after a connection switch', async () => {
    const bridge = controllableBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();

    const oldMetadata = deferred<MetadataNode[]>();
    vi.mocked(bridge.loadMetadata).mockImplementation(async (connectionId) => connectionId === postgres.id ? oldMetadata.promise : mongoRoot);
    const refreshOld = store.getState().loadNode(null, true);

    const oldQuery = deferred<QueryResult>();
    vi.mocked(bridge.executeQuery).mockImplementation(async (request) => request.connectionId === postgres.id ? oldQuery.promise : result('mongo'));
    const runOld = store.getState().runQuery();

    await store.getState().setActiveConnection(mongo.id);
    await store.getState().runQuery();
    expect(store.getState().metadata.root).toEqual(mongoRoot);
    expect(store.getState().result?.rows[0]).toEqual({ engine: 'mongo' });

    oldMetadata.resolve([{ id: 'stale', parentId: null, name: 'stale-postgres', kind: 'database' }]);
    oldQuery.resolve(result('stale-postgres'));
    await Promise.all([refreshOld, runOld]);

    expect(store.getState().metadata.root).toEqual(mongoRoot);
    expect(store.getState().result?.rows[0]).toEqual({ engine: 'mongo' });
    expect(store.getState().queryStatus).toBe('success');
  });

  it('ignores an older completion when the same tab is run again', async () => {
    const bridge = controllableBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    const first = deferred<QueryResult>();
    vi.mocked(bridge.executeQuery)
      .mockImplementationOnce(async () => first.promise)
      .mockImplementationOnce(async () => result('newest'));

    const oldRun = store.getState().runQuery();
    await store.getState().runQuery('SELECT newer');
    first.resolve(result('oldest'));
    await oldRun;
    expect(store.getState().result?.rows[0]).toEqual({ engine: 'newest' });
  });

  it('does not overwrite same-tab edits with an in-flight query completion', async () => {
    const bridge = controllableBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    const pending = deferred<QueryResult>();
    vi.mocked(bridge.executeQuery).mockImplementationOnce(async () => pending.promise);
    const run = store.getState().runQuery();
    store.getState().updateQuery('SELECT edited_while_running');
    pending.resolve(result('stale'));
    await run;
    const tab = store.getState().tabs.find((item) => item.id === store.getState().activeTabId)!;
    expect(tab.query).toBe('SELECT edited_while_running');
    expect(tab.dirty).toBe(true);
    expect(store.getState().result).toBeNull();
    expect(store.getState().queryStatus).toBe('idle');
  });

  it('publishes only the newest metadata refresh for a connection and node', async () => {
    const bridge = controllableBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    const first = deferred<MetadataNode[]>();
    const second = deferred<MetadataNode[]>();
    vi.mocked(bridge.loadMetadata)
      .mockImplementationOnce(async () => first.promise)
      .mockImplementationOnce(async () => second.promise);
    const firstRefresh = store.getState().loadNode(null, true);
    const secondRefresh = store.getState().loadNode(null, true);
    second.resolve([{ id: 'new', parentId: null, name: 'newest', kind: 'database' }]);
    await secondRefresh;
    first.resolve([{ id: 'old', parentId: null, name: 'oldest', kind: 'database' }]);
    await firstRefresh;
    expect(store.getState().metadata.root).toEqual([expect.objectContaining({ name: 'newest' })]);
  });

  it('preserves edits staged while an earlier mutation batch is applying', async () => {
    const bridge = new DemoBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().runQuery();
    store.getState().stageCell(0, 'plan', 'Enterprise');
    const pending = deferred<{ applied: number; message: string }>();
    vi.spyOn(bridge, 'applyMutations').mockImplementationOnce(async () => pending.promise);
    const applying = store.getState().applyMutations();
    store.getState().stageCell(0, 'plan', 'Professional');
    pending.resolve({ applied: 1, message: 'Applied' });
    await applying;
    expect(store.getState().mutations).toEqual([expect.objectContaining({ column: 'plan', nextValue: 'Professional' })]);
    expect(store.getState().changesOpen).toBe(true);
    expect(store.getState().result?.rows[0].plan).toBe('Enterprise');
  });

  it('does not include result rows in AI transport context', async () => {
    const bridge = controllableBridge();
    vi.mocked(bridge.askAi).mockResolvedValue({ intent: 'guidance', message: 'ok' });
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().runQuery();
    await store.getState().askAi('Explain the active query');
    expect(bridge.askAi).toHaveBeenCalledWith(expect.objectContaining({
      connectionId: postgres.id,
      databaseKind: 'postgresql',
      language: 'sql',
      query: expect.stringContaining('public.customers'),
    }));
    expect(vi.mocked(bridge.askAi).mock.calls[0][0]).not.toHaveProperty('resultPreview');
  });
});
