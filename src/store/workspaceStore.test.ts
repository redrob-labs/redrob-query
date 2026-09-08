import { describe, expect, it, vi } from 'vitest';
import { DemoBridge, type DataBridge } from '../api/bridge';
import type { ConnectionProfile, MetadataNode, QueryResult } from '../domain/types';
import { createWorkspaceStore } from './workspaceStore';
import { MAX_HISTORY_ENTRIES, MAX_PERSISTED_TABS, WORKSPACE_STORAGE_KEY } from './workspacePersistence';

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
    profileWarnings: vi.fn(async () => []),
    saveConnection: vi.fn(),
    removeConnection: vi.fn(async () => ({})),
    connect: vi.fn(async () => ({ ok: true, message: 'Connected' })),
    disconnect: vi.fn(),
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
    const profile = (await bridge.saveConnection({ name: 'Documents', kind: 'mongodb', host: 'localhost', port: 27017, database: 'accounts', username: 'reader', tls: true, authSource: 'admin' })).profile;
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
    const profile = (await bridge.saveConnection({ name: 'Local', kind: 'sqlite', host: '', port: 0, database: '', username: '', filePath: '/tmp/local.sqlite', tls: true })).profile;
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

  it('loads metadata when a connecting profile is reselected before completion', async () => {
    const bridge = controllableBridge('desktop');
    vi.mocked(bridge.listConnections).mockResolvedValueOnce([{ ...postgres, state: 'disconnected' }, mongo]);
    const pending = deferred<{ ok: boolean; message: string }>();
    vi.mocked(bridge.connect).mockImplementationOnce(async () => pending.promise);
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);

    const connecting = store.getState().connectConnection(postgres.id);
    await store.getState().setActiveConnection(postgres.id);
    pending.resolve({ ok: true, message: 'Connected' });
    await connecting;

    expect(store.getState().activeConnectionId).toBe(postgres.id);
    expect(store.getState().connections.find((item) => item.id === postgres.id)?.state).toBe('connected');
    expect(store.getState().metadata.root).toEqual(pgRoot);
    expect(bridge.loadMetadata).toHaveBeenCalledWith(postgres.id, null);
  });

  it('serializes connect then remove so the committed removal remains authoritative', async () => {
    const bridge = controllableBridge('desktop');
    const pendingConnect = deferred<{ ok: boolean; message: string }>();
    vi.mocked(bridge.connect).mockImplementationOnce(async () => pendingConnect.promise);
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();

    const connecting = store.getState().connectConnection(postgres.id);
    await vi.waitFor(() => expect(bridge.connect).toHaveBeenCalledOnce());
    const removing = store.getState().removeConnection(postgres.id);
    expect(bridge.removeConnection).not.toHaveBeenCalled();
    pendingConnect.resolve({ ok: true, message: 'Connected' });
    await Promise.all([connecting, removing]);

    expect(bridge.removeConnection).toHaveBeenCalledWith(postgres.id);
    expect(store.getState().connections.some((profile) => profile.id === postgres.id)).toBe(false);
  });

  it('serializes remove then connect without reconnecting a deleted profile', async () => {
    const bridge = controllableBridge('desktop');
    const pendingRemove = deferred<Record<string, never>>();
    vi.mocked(bridge.removeConnection).mockImplementationOnce(async () => pendingRemove.promise);
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();

    const removing = store.getState().removeConnection(postgres.id);
    await vi.waitFor(() => expect(bridge.removeConnection).toHaveBeenCalledOnce());
    const connecting = store.getState().connectConnection(postgres.id);
    expect(bridge.connect).not.toHaveBeenCalled();
    pendingRemove.resolve({});
    await Promise.all([removing, connecting]);

    expect(bridge.connect).not.toHaveBeenCalled();
    expect(store.getState().connections.some((profile) => profile.id === postgres.id)).toBe(false);
  });

  it('serializes save then remove in the same order as the native profile lifecycle lock', async () => {
    const bridge = controllableBridge('desktop');
    const pendingSave = deferred<{ profile: ConnectionProfile }>();
    vi.mocked(bridge.saveConnection).mockImplementationOnce(async () => pendingSave.promise);
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    const draft = { id: postgres.id, name: 'Renamed', kind: 'postgresql' as const, host: 'localhost', port: 5432, database: 'app', username: 'reader', tls: true };

    const saving = store.getState().saveConnection(draft);
    await vi.waitFor(() => expect(bridge.saveConnection).toHaveBeenCalledOnce());
    const removing = store.getState().removeConnection(postgres.id);
    expect(bridge.removeConnection).not.toHaveBeenCalled();
    pendingSave.resolve({ profile: { ...postgres, name: 'Renamed', state: 'disconnected' } });
    await Promise.all([saving, removing]);

    expect(bridge.removeConnection).toHaveBeenCalledWith(postgres.id);
    expect(store.getState().connections.some((profile) => profile.id === postgres.id)).toBe(false);
  });

  it('serializes remove then save and publishes the later native recreation', async () => {
    const bridge = controllableBridge('desktop');
    const pendingRemove = deferred<Record<string, never>>();
    vi.mocked(bridge.removeConnection).mockImplementationOnce(async () => pendingRemove.promise);
    vi.mocked(bridge.saveConnection).mockResolvedValueOnce({ profile: { ...postgres, name: 'Recreated', state: 'disconnected' } });
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    const draft = { id: postgres.id, name: 'Recreated', kind: 'postgresql' as const, host: 'localhost', port: 5432, database: 'app', username: 'reader', tls: true };

    const removing = store.getState().removeConnection(postgres.id);
    await vi.waitFor(() => expect(bridge.removeConnection).toHaveBeenCalledOnce());
    const saving = store.getState().saveConnection(draft);
    expect(bridge.saveConnection).not.toHaveBeenCalled();
    pendingRemove.resolve({});
    await Promise.all([removing, saving]);

    expect(bridge.saveConnection).toHaveBeenCalledWith(draft);
    expect(store.getState().connections).toContainEqual(expect.objectContaining({ id: postgres.id, name: 'Recreated' }));
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

  it('does not overwrite restored history with an in-flight query completion', async () => {
    const bridge = controllableBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().runQuery();
    const entry = store.getState().queryHistory[0];
    const pending = deferred<QueryResult>();
    vi.mocked(bridge.executeQuery).mockImplementationOnce(async () => pending.promise);

    const run = store.getState().runQuery('SELECT stale_pending');
    store.getState().restoreHistory(entry.id);
    pending.resolve(result('stale'));
    await run;

    const state = store.getState();
    const tab = state.tabs.find((item) => item.id === state.activeTabId)!;
    expect(tab.query).toBe(entry.query);
    expect(tab.dirty).toBe(true);
    expect(state.result).toBeNull();
    expect(state.queryStatus).toBe('idle');
    expect(state.mutations).toEqual([]);
    expect(state.mutationStatus).toBe('idle');
    expect(state.changesOpen).toBe(false);
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



describe('release workspace contracts', () => {
  it('restores bounded desktop query-only state without selecting, connecting, or running', async () => {
    const tabs = Array.from({ length: 35 }, (_, index) => ({ id: `saved-${index}`, connectionId: postgres.id, name: `Saved ${index}`, language: 'sql', query: `SELECT ${index}`, dirty: true, credentials: 'must-not-restore' }));
    const history = Array.from({ length: 55 }, (_, index) => ({ id: `history-${index}`, connectionId: postgres.id, name: 'Saved', language: 'sql', query: `SELECT ${index}`, executedAt: index, rows: [{ secret: true }], aiPrompt: 'private' }));
    localStorage.setItem(WORKSPACE_STORAGE_KEY, JSON.stringify({ version: 1, tabs, history, credentials: 'top-secret' }));
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    expect(store.getState().tabs).toHaveLength(MAX_PERSISTED_TABS);
    expect(store.getState().queryHistory).toHaveLength(MAX_HISTORY_ENTRIES);
    expect(store.getState().activeConnectionId).toBeNull();
    expect(bridge.connect).not.toHaveBeenCalled();
    expect(bridge.executeQuery).not.toHaveBeenCalled();

    await store.getState().setActiveConnection(postgres.id);
    store.getState().updateQuery('SELECT persisted_immediately');
    const stored = localStorage.getItem(WORKSPACE_STORAGE_KEY)!;
    expect(stored).toContain('persisted_immediately');
    expect(stored).not.toContain('top-secret');
    expect(stored).not.toContain('must-not-restore');
    expect(stored).not.toContain('aiPrompt');
    expect(stored).not.toContain('rows');
  });

  it('preserves an empty tab draft, unrelated tabs, and nonempty history across reloads', async () => {
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);
    await store.getState().runQuery();
    await store.getState().setActiveConnection(mongo.id);
    store.getState().updateQuery('{"collection":"customers","operation":"find","filter":{"kept":true}}');
    await store.getState().setActiveConnection(postgres.id);

    store.getState().updateQuery('');

    const stored = JSON.parse(localStorage.getItem(WORKSPACE_STORAGE_KEY)!) as {
      tabs: Array<{ connectionId: string; query: string }>;
      history: Array<{ connectionId: string; query: string }>;
    };
    expect(stored.tabs).toEqual(expect.arrayContaining([
      expect.objectContaining({ connectionId: postgres.id, query: '' }),
      expect.objectContaining({ connectionId: mongo.id, query: expect.stringContaining('"kept":true') }),
    ]));
    expect(stored.history).toEqual([expect.objectContaining({ connectionId: postgres.id, query: expect.stringContaining('SELECT') })]);

    const reloaded = createWorkspaceStore(controllableBridge('desktop'));
    await reloaded.getState().initialize();
    expect(reloaded.getState().tabs).toEqual(expect.arrayContaining([
      expect.objectContaining({ connectionId: postgres.id, query: '' }),
      expect.objectContaining({ connectionId: mongo.id, query: expect.stringContaining('"kept":true') }),
    ]));
    expect(reloaded.getState().queryHistory).toEqual([expect.objectContaining({ connectionId: postgres.id, query: expect.stringContaining('SELECT') })]);
    expect(reloaded.getState().startupWarnings).toEqual([]);
  });

  it('records successful first-page history and drives page offsets and size', async () => {
    const bridge = controllableBridge('desktop');
    vi.mocked(bridge.executeQuery).mockImplementation(async (query) => ({
      ...result(query.connectionId), offset: query.offset ?? 0, limit: query.limit ?? 50,
      nextOffset: (query.offset ?? 0) === 0 ? (query.limit ?? 50) : null, truncated: (query.offset ?? 0) === 0,
    }));
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);
    await store.getState().runQuery();
    expect(store.getState().queryHistory).toHaveLength(1);
    expect(bridge.executeQuery).toHaveBeenLastCalledWith(expect.objectContaining({ limit: 50, offset: 0 }));
    await store.getState().nextPage();
    expect(store.getState().pageOffset).toBe(50);
    expect(store.getState().queryHistory).toHaveLength(1);
    await store.getState().previousPage();
    expect(store.getState().pageOffset).toBe(0);
    await store.getState().setPageSize(25);
    expect(store.getState().pageSize).toBe(25);
    expect(bridge.executeQuery).toHaveBeenLastCalledWith(expect.objectContaining({ limit: 25, offset: 0 }));
    const entry = store.getState().queryHistory[0];
    store.getState().updateQuery('SELECT changed');
    store.getState().restoreHistory(entry.id);
    expect(store.getState().tabs.find((tab) => tab.id === store.getState().activeTabId)?.query).toBe(entry.query);
    store.getState().clearHistory(postgres.id);
    expect(store.getState().queryHistory).toEqual([]);
  });

  it('clears only the selected connection history and preserves other persisted history', async () => {
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);
    store.getState().updateQuery('SELECT pg_history');
    await store.getState().runQuery();
    await store.getState().setActiveConnection(mongo.id);
    store.getState().updateQuery('{"collection":"customers","operation":"find","filter":{"marker":"mongo_history"}}');
    await store.getState().runQuery();

    store.getState().clearHistory(postgres.id);

    expect(store.getState().queryHistory).toEqual([expect.objectContaining({ connectionId: mongo.id })]);
    const stored = JSON.parse(localStorage.getItem(WORKSPACE_STORAGE_KEY)!) as { history: Array<{ connectionId: string; query: string }> };
    expect(stored.history).toEqual([expect.objectContaining({ connectionId: mongo.id })]);
    expect(JSON.stringify(stored.history)).not.toContain('pg_history');
  });

  it('reconciles a committed removal before showing its cleanup warning', async () => {
    const bridge = controllableBridge('desktop');
    vi.mocked(bridge.removeConnection).mockResolvedValueOnce({ warning: 'Credential cleanup is pending.' });
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);
    await store.getState().runQuery();

    await store.getState().removeConnection(postgres.id);

    expect(store.getState().connections.some((profile) => profile.id === postgres.id)).toBe(false);
    expect(store.getState().tabs.some((tab) => tab.connectionId === postgres.id)).toBe(false);
    expect(store.getState().queryHistory.some((entry) => entry.connectionId === postgres.id)).toBe(false);
    expect(store.getState().toasts.at(-1)).toEqual(expect.objectContaining({
      tone: 'info',
      title: 'Connection removed with a warning',
      detail: 'Credential cleanup is pending.',
    }));
  });

  it('removes active and inactive profile history and queries from persisted workspace state', async () => {
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);
    store.getState().updateQuery('SELECT removed_pg_private_text');
    await store.getState().runQuery();
    await store.getState().setActiveConnection(mongo.id);
    store.getState().updateQuery('{"collection":"customers","operation":"find","filter":{"marker":"removed_mongo_private_text"}}');
    await store.getState().runQuery();
    const mongoResult = store.getState().result;

    await store.getState().removeConnection(postgres.id);

    expect(store.getState().activeConnectionId).toBe(mongo.id);
    expect(store.getState().result).toBe(mongoResult);
    expect(store.getState().tabs.some((tab) => tab.connectionId === postgres.id)).toBe(false);
    expect(store.getState().queryHistory.every((entry) => entry.connectionId === mongo.id)).toBe(true);
    let stored = localStorage.getItem(WORKSPACE_STORAGE_KEY)!;
    expect(stored).not.toContain(postgres.id);
    expect(stored).not.toContain('removed_pg_private_text');
    expect(stored).toContain('removed_mongo_private_text');

    await store.getState().removeConnection(mongo.id);

    expect(store.getState()).toEqual(expect.objectContaining({ activeConnectionId: null, activeTabId: '', result: null, queryHistory: [] }));
    stored = localStorage.getItem(WORKSPACE_STORAGE_KEY)!;
    expect(stored).not.toContain(mongo.id);
    expect(stored).not.toContain('removed_mongo_private_text');
  });

  it('preserves query tabs and history across active and inactive disconnects', async () => {
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);
    await store.getState().runQuery();
    store.getState().updateQuery('SELECT dirty_draft_survives_disconnect');
    const postgresTabId = store.getState().activeTabId;

    await store.getState().disconnectConnection(postgres.id);

    expect(store.getState()).toEqual(expect.objectContaining({
      activeConnectionId: postgres.id,
      activeTabId: postgresTabId,
      result: null,
      mutations: [],
    }));
    expect(store.getState().connections.find((item) => item.id === postgres.id)?.state).toBe('disconnected');
    expect(store.getState().tabs.find((tab) => tab.id === postgresTabId)?.query).toBe('SELECT dirty_draft_survives_disconnect');
    expect(store.getState().queryHistory).toHaveLength(1);
    expect(localStorage.getItem(WORKSPACE_STORAGE_KEY)).toContain('dirty_draft_survives_disconnect');

    await store.getState().connectConnection(postgres.id);
    expect(store.getState().connections.find((item) => item.id === postgres.id)?.state).toBe('connected');
    expect(store.getState().tabs.find((tab) => tab.id === postgresTabId)?.query).toBe('SELECT dirty_draft_survives_disconnect');
    expect(store.getState().metadata.root).toEqual(pgRoot);

    await store.getState().setActiveConnection(mongo.id);
    await store.getState().runQuery();
    const mongoResult = store.getState().result;
    await store.getState().disconnectConnection(postgres.id);
    expect(store.getState().activeConnectionId).toBe(mongo.id);
    expect(store.getState().result).toBe(mongoResult);
    expect(store.getState().tabs.find((tab) => tab.id === postgresTabId)?.query).toBe('SELECT dirty_draft_survives_disconnect');
  });

  it('keeps staged edits when a bridge reports a partial mutation application', async () => {
    const bridge = new DemoBridge();
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().runQuery();
    store.getState().stageCell(0, 'plan', 'Enterprise');
    vi.spyOn(bridge, 'applyMutations').mockResolvedValueOnce({ applied: 0, message: 'Conflict' });
    await store.getState().applyMutations();
    expect(store.getState().mutations).toHaveLength(1);
    expect(store.getState().result?.rows[0].plan).toBe('Scale');
    expect(store.getState().mutationStatus).toBe('error');
  });

  it('stores sanitized startup warnings persistently in state', async () => {
    const bridge = controllableBridge('desktop');
    vi.mocked(bridge.profileWarnings).mockResolvedValue(['bad\u0000 profile\npath']);
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    expect(store.getState().startupWarnings).toEqual(['bad profile path']);
  });
});



describe('profile edit state synchronization', () => {
  it('clears active results and disables execution when an edit returns disconnected', async () => {
    const bridge = controllableBridge('desktop');
    const store = createWorkspaceStore(bridge);
    await store.getState().initialize();
    await store.getState().setActiveConnection(postgres.id);
    await store.getState().runQuery();
    await store.getState().addConnection({ ...postgres, name: 'Renamed', state: 'disconnected' });
    expect(store.getState().result).toBeNull();
    expect(store.getState().connections.find((item) => item.id === postgres.id)?.state).toBe('disconnected');
    vi.mocked(bridge.executeQuery).mockClear();
    await store.getState().runQuery();
    expect(bridge.executeQuery).not.toHaveBeenCalled();
  });
});
