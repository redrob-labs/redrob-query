import { create, type StoreApi, type UseBoundStore } from 'zustand';
import type { DataBridge } from '../api/bridge';
import type {
  AiResponse,
  AsyncStatus,
  CellMutation,
  CellValue,
  ConnectionDraft,
  ConnectionProfile,
  DatabaseKind,
  MetadataNode,
  QueryHistoryEntry,
  QueryLanguage,
  QueryResult,
  QueryTab,
  ToastMessage,
} from '../domain/types';
import { clearWorkspace, loadWorkspace, loadWorkspacePersistencePreference, MAX_HISTORY_ENTRIES, saveWorkspace, setWorkspacePersistence } from './workspacePersistence';

const postgresStarter = `SELECT
  id,
  name,
  email,
  plan,
  mrr,
  active,
  created_at
FROM public.customers
ORDER BY created_at DESC
LIMIT 250;`;
const sqliteStarter = `SELECT
  id,
  name,
  email,
  city,
  created_at
FROM customers
ORDER BY created_at DESC
LIMIT 250;`;
const mongoStarter = `{
  "collection": "customers",
  "operation": "find",
  "filter": {},
  "limit": 250
}`;

const starterForKind = (kind: DatabaseKind): { language: QueryLanguage; query: string; name: string } => {
  if (kind === 'mongodb') return { language: 'mql', query: mongoStarter, name: 'Customer documents' };
  if (kind === 'sqlite') return { language: 'sql', query: sqliteStarter, name: 'Customer overview' };
  if (kind === 'postgresql') return { language: 'sql', query: postgresStarter, name: 'Customer overview' };
  return { language: 'sql', query: 'SELECT *\nFROM customers\nLIMIT 250;', name: 'Customer overview' };
};
const makeStarterTab = (connection: ConnectionProfile, id: string): QueryTab => ({ id, connectionId: connection.id, ...starterForKind(connection.kind), dirty: false });
interface AiMessage { role: 'user' | 'assistant'; content: string; query?: string; language?: QueryLanguage }

interface WorkspaceState {
  bridge: DataBridge;
  initialized: boolean;
  startupWarnings: string[];
  connections: ConnectionProfile[];
  activeConnectionId: string | null;
  metadata: Record<string, MetadataNode[]>;
  expandedNodes: Set<string>;
  tabs: QueryTab[];
  activeTabId: string;
  queryHistory: QueryHistoryEntry[];
  localPersistenceEnabled: boolean;
  localPersistenceStatus: 'disabled' | 'saved' | 'error';
  localPersistenceMessage: string | null;
  result: QueryResult | null;
  resultRevision: number;
  pageSize: 25 | 50 | 100 | 250;
  pageOffset: number;
  queryStatus: AsyncStatus;
  queryError: string | null;
  mutations: CellMutation[];
  mutationStatus: AsyncStatus;
  aiMessages: AiMessage[];
  aiStatus: AsyncStatus;
  aiOpen: boolean;
  aiSettingsOpen: boolean;
  aiWidth: number;
  navigatorOpen: boolean;
  navigatorSearchRequest: number;
  changesOpen: boolean;
  connectionModalOpen: boolean;
  connectionModalProfileId: string | null;
  commandPaletteOpen: boolean;
  toasts: ToastMessage[];
  initialize(): Promise<void>;
  loadNode(parentId?: string | null, force?: boolean): Promise<void>;
  toggleNode(node: MetadataNode): Promise<void>;
  setActiveConnection(id: string): Promise<void>;
  connectConnection(id: string): Promise<void>;
  disconnectConnection(id: string): Promise<void>;
  saveConnection(draft: ConnectionDraft): Promise<void>;
  removeConnection(id: string): Promise<void>;
  openConnectionModal(profileId?: string): void;
  updateQuery(query: string): void;
  runQuery(queryOverride?: string, offsetOverride?: number): Promise<void>;
  nextPage(): Promise<void>;
  previousPage(): Promise<void>;
  setPageSize(size: 25 | 50 | 100 | 250): Promise<void>;
  newTab(language?: QueryLanguage): void;
  setTabLanguage(language: QueryLanguage): void;
  closeTab(id: string): void;
  setActiveTab(id: string): void;
  restoreHistory(id: string): void;
  clearHistory(connectionId: string): void;
  clearLocalWorkspace(): void;
  enableLocalWorkspace(): void;
  focusNavigatorSearch(): void;
  stageCell(rowIndex: number, column: string, value: CellValue): void;
  discardMutation(id: string): void;
  discardAllMutations(): void;
  applyMutations(): Promise<void>;
  askAi(prompt: string): Promise<AiResponse | null>;
  useGeneratedQuery(query: string): void;
  addConnection(profile: ConnectionProfile): Promise<void>;
  notify(tone: ToastMessage['tone'], title: string, detail?: string): void;
  setUi(values: Partial<Pick<WorkspaceState, 'aiOpen' | 'aiSettingsOpen' | 'aiWidth' | 'navigatorOpen' | 'changesOpen' | 'connectionModalOpen' | 'connectionModalProfileId' | 'commandPaletteOpen'>>): void;
  dismissToast(id: string): void;
}

const asMessage = (error: unknown) => error instanceof Error ? error.message : 'Something went wrong.';
const toastId = () => `toast-${Date.now()}-${Math.random().toString(16).slice(2)}`;
const sanitizeWarning = (warning: string) => warning.replace(/[\u0000-\u001f\u007f]/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 500);
const welcomeMessage = (mode: DataBridge['mode']): AiMessage => ({ role: 'assistant', content: mode === 'demo' ? 'I’m ready to explore the active demo data with you. Ask for a query or an explanation.' : 'I’m ready to help with the active connection. Ask for a query or an explanation.' });

export const createWorkspaceStore = (bridge: DataBridge): UseBoundStore<StoreApi<WorkspaceState>> => {
  let tabSequence = 0;
  let metadataGeneration = 0;
  let queryGeneration = 0;
  let mutationGeneration = 0;
  let aiGeneration = 0;
  const lifecycleRequests = new Map<string, number>();
  const metadataRequests = new Map<string, number>();
  let lifecycleQueue: Promise<unknown> = Promise.resolve();
  const enqueueLifecycle = <T>(operation: () => Promise<T>): Promise<T> => {
    const result = lifecycleQueue.then(operation, operation);
    lifecycleQueue = result.then(() => undefined, () => undefined);
    return result;
  };
  const nextLifecycleRequest = (id: string) => { const token = (lifecycleRequests.get(id) ?? 0) + 1; lifecycleRequests.set(id, token); return token; };
  const nextTabId = () => `query-${++tabSequence}`;

  return create<WorkspaceState>((set, get) => {
    const persist = () => {
      const state = get();
      if (bridge.mode !== 'desktop' || !state.localPersistenceEnabled) return;
      const saved = saveWorkspace(state.tabs, state.queryHistory);
      set({ localPersistenceStatus: saved.ok ? 'saved' : 'error', localPersistenceMessage: saved.message ?? null });
    };
    const clearProfile = (connectionId: string, removeProfile = false) => {
      const wasActive = get().activeConnectionId === connectionId;
      if (wasActive) { metadataGeneration += 1; queryGeneration += 1; mutationGeneration += 1; aiGeneration += 1; }
      set((state) => ({
        connections: removeProfile ? state.connections.filter((item) => item.id !== connectionId) : state.connections,
        activeConnectionId: removeProfile && wasActive ? null : state.activeConnectionId,
        metadata: wasActive ? {} : state.metadata,
        expandedNodes: wasActive ? new Set() : state.expandedNodes,
        tabs: removeProfile ? state.tabs.filter((tab) => tab.connectionId !== connectionId) : state.tabs,
        activeTabId: removeProfile && wasActive ? '' : state.activeTabId,
        queryHistory: removeProfile ? state.queryHistory.filter((entry) => entry.connectionId !== connectionId) : state.queryHistory,
        result: wasActive ? null : state.result,
        pageOffset: wasActive ? 0 : state.pageOffset,
        queryStatus: wasActive ? 'idle' : state.queryStatus,
        queryError: wasActive ? null : state.queryError,
        mutations: wasActive ? [] : state.mutations,
        mutationStatus: wasActive ? 'idle' : state.mutationStatus,
        changesOpen: wasActive ? false : state.changesOpen,
        aiStatus: wasActive ? 'idle' : state.aiStatus,
      }));
      persist();
    };
    return {
      bridge, initialized: false, startupWarnings: [], connections: [], activeConnectionId: null,
      metadata: {}, expandedNodes: new Set(), tabs: [], activeTabId: '', queryHistory: [],
      localPersistenceEnabled: false, localPersistenceStatus: 'disabled', localPersistenceMessage: null,
      result: null, resultRevision: 0,
      pageSize: 50, pageOffset: 0, queryStatus: 'idle', queryError: null, mutations: [], mutationStatus: 'idle',
      aiMessages: [welcomeMessage(bridge.mode)], aiStatus: 'idle', aiOpen: true, aiSettingsOpen: false, aiWidth: 330,
      navigatorOpen: true, navigatorSearchRequest: 0, changesOpen: false, connectionModalOpen: false, connectionModalProfileId: null,
      commandPaletteOpen: false, toasts: [],

      async initialize() {
        if (get().initialized) return;
        const warningRequest = bridge.profileWarnings()
          .then((warnings) => warnings.map(sanitizeWarning).filter(Boolean))
          .catch(() => ['Saved-profile diagnostics are temporarily unavailable.']);
        try {
          const connections = await bridge.listConnections();
          const profileWarnings = await warningRequest;
          const preference = bridge.mode === 'desktop' ? loadWorkspacePersistencePreference() : { ok: true, enabled: false };
          const disabledCleanup = bridge.mode === 'desktop' && preference.ok && !preference.enabled ? clearWorkspace() : { ok: true };
          const restored = bridge.mode === 'desktop' && preference.enabled ? loadWorkspace(connections) : { tabs: [], history: [] };
          const startupWarnings = [...profileWarnings, ...(preference.message ? [preference.message] : []), ...(disabledCleanup.message ? [disabledCleanup.message] : []), ...(restored.warning ? [restored.warning] : [])];
          const persistenceError = preference.ok ? disabledCleanup.ok ? restored.storageError : disabledCleanup.message : preference.message;
          const restoredSequence = restored.tabs.reduce((max, tab) => Math.max(max, Number(tab.id.match(/^query-(\d+)$/)?.[1] ?? 0)), 0);
          tabSequence = Math.max(tabSequence, restoredSequence);
          const activeConnection = bridge.mode === 'demo' ? connections.find((item) => item.state === 'connected') : undefined;
          const starter = activeConnection ? makeStarterTab(activeConnection, nextTabId()) : null;
          set({
            connections, startupWarnings, tabs: starter ? [starter] : restored.tabs, queryHistory: restored.history,
            localPersistenceEnabled: Boolean(preference.enabled && !persistenceError),
            localPersistenceStatus: persistenceError ? 'error' : preference.enabled ? 'saved' : 'disabled',
            localPersistenceMessage: persistenceError ?? null,
            activeConnectionId: activeConnection?.id ?? null, activeTabId: starter?.id ?? '', initialized: true,
          });
          if (activeConnection) await get().loadNode(null);
        } catch (error) {
          set({ initialized: true, queryError: asMessage(error), queryStatus: 'error' });
        }
      },

      async loadNode(parentId = null, force = false) {
        const connectionId = get().activeConnectionId;
        const connection = get().connections.find((item) => item.id === connectionId);
        if (!connectionId || connection?.state !== 'connected') return;
        const generation = metadataGeneration;
        const key = parentId ?? 'root';
        if (!force && get().metadata[key]) return;
        const requestKey = `${connectionId}:${key}`;
        const requestToken = (metadataRequests.get(requestKey) ?? 0) + 1;
        metadataRequests.set(requestKey, requestToken);
        if (force) set((state) => { const metadata = { ...state.metadata }; delete metadata[key]; return { metadata }; });
        try {
          const nodes = await bridge.loadMetadata(connectionId, parentId);
          if (get().activeConnectionId !== connectionId || metadataGeneration !== generation || metadataRequests.get(requestKey) !== requestToken) return;
          set((state) => ({ metadata: { ...state.metadata, [key]: nodes } }));
        } catch (error) {
          if (get().activeConnectionId !== connectionId || metadataGeneration !== generation || metadataRequests.get(requestKey) !== requestToken) return;
          set((state) => ({ toasts: [...state.toasts, { id: toastId(), tone: 'error', title: 'Could not load schema', detail: asMessage(error) }] }));
        }
      },

      async toggleNode(node) {
        const expandedNodes = new Set(get().expandedNodes);
        if (expandedNodes.has(node.id)) expandedNodes.delete(node.id);
        else { expandedNodes.add(node.id); set({ expandedNodes }); if (node.childCount) await get().loadNode(node.id); return; }
        set({ expandedNodes });
      },

      async setActiveConnection(id) {
        const connection = get().connections.find((item) => item.id === id);
        if (!connection) return;
        metadataGeneration += 1; queryGeneration += 1; mutationGeneration += 1; aiGeneration += 1;
        const existingTab = get().tabs.find((tab) => tab.connectionId === id);
        const tab = existingTab ?? makeStarterTab(connection, nextTabId());
        set((state) => ({
          activeConnectionId: id, metadata: {}, expandedNodes: new Set(), tabs: existingTab ? state.tabs : [...state.tabs, tab], activeTabId: tab.id,
          result: null, pageOffset: 0, queryStatus: 'idle', queryError: null, mutations: [], mutationStatus: 'idle', changesOpen: false,
          aiMessages: [welcomeMessage(bridge.mode)], aiStatus: 'idle',
        }));
        persist();
        if (connection.state === 'connected') await get().loadNode(null);
      },

      connectConnection(id) {
        return enqueueLifecycle(async () => {
          if (!get().connections.some((item) => item.id === id)) return;
          const requestToken = nextLifecycleRequest(id);
          set((state) => ({ connections: state.connections.map((item) => item.id === id ? { ...item, state: 'connecting' } : item) }));
          try {
            const status = await bridge.connect(id);
            if (lifecycleRequests.get(id) !== requestToken || !get().connections.some((item) => item.id === id)) return;
            set((state) => ({ connections: state.connections.map((item) => item.id === id ? { ...item, state: status.ok ? 'connected' : 'error' } : item) }));
            if (!status.ok) { get().notify('error', 'Could not connect', status.message); return; }
            if (get().activeConnectionId === id) await get().loadNode(null, true);
          } catch (error) {
            if (lifecycleRequests.get(id) !== requestToken || !get().connections.some((item) => item.id === id)) return;
            set((state) => ({ connections: state.connections.map((item) => item.id === id ? { ...item, state: 'error' } : item) }));
            get().notify('error', 'Could not connect', asMessage(error));
          }
        });
      },

      disconnectConnection(id) {
        return enqueueLifecycle(async () => {
          if (!get().connections.some((item) => item.id === id)) return;
          const requestToken = nextLifecycleRequest(id);
          try {
            await bridge.disconnect(id);
            if (lifecycleRequests.get(id) !== requestToken || !get().connections.some((item) => item.id === id)) return;
            set((state) => ({ connections: state.connections.map((item) => item.id === id ? { ...item, state: 'disconnected' } : item) }));
            clearProfile(id);
          } catch (error) {
            if (lifecycleRequests.get(id) === requestToken && get().connections.some((item) => item.id === id)) get().notify('error', 'Could not disconnect', asMessage(error));
          }
        });
      },

      saveConnection(draft) {
        return enqueueLifecycle(async () => {
          const outcome = await bridge.saveConnection(draft);
          await get().addConnection(outcome.profile);
          if (outcome.warning) get().notify('info', 'Connection saved with a warning', outcome.warning);
        });
      },

      removeConnection(id) {
        return enqueueLifecycle(async () => {
          const profile = get().connections.find((item) => item.id === id);
          if (!profile || profile.builtIn) return;
          const requestToken = nextLifecycleRequest(id);
          try {
            const outcome = await bridge.removeConnection(id);
            if (lifecycleRequests.get(id) !== requestToken) return;
            clearProfile(id, true);
            if (outcome.warning) get().notify('info', 'Connection removed with a warning', outcome.warning);
          } catch (error) {
            if (lifecycleRequests.get(id) === requestToken && get().connections.some((item) => item.id === id)) get().notify('error', 'Could not remove connection', asMessage(error));
          }
        });
      },

      openConnectionModal(profileId) { set({ connectionModalOpen: true, connectionModalProfileId: profileId ?? null }); },

      updateQuery(query) {
        queryGeneration += 1; mutationGeneration += 1;
        set((state) => ({
          tabs: state.tabs.map((tab) => tab.id === state.activeTabId ? { ...tab, query, dirty: true } : tab),
          result: null, pageOffset: 0, queryStatus: 'idle', queryError: null,
          mutations: [], mutationStatus: 'idle', changesOpen: false,
        }));
        persist();
      },

      async runQuery(queryOverride, offsetOverride = 0) {
        const state = get();
        const tab = state.tabs.find((item) => item.id === state.activeTabId);
        const connectionId = state.activeConnectionId;
        const connection = state.connections.find((item) => item.id === connectionId);
        if (!tab || !connectionId || tab.connectionId !== connectionId || connection?.state !== 'connected') return;
        const query = queryOverride ?? tab.query;
        const offset = Math.max(0, offsetOverride);
        const generation = ++queryGeneration;
        mutationGeneration += 1;
        set({ queryStatus: 'loading', queryError: null, mutations: [], mutationStatus: 'idle', changesOpen: false });
        try {
          const result = await bridge.executeQuery({ connectionId, query, language: tab.language, limit: state.pageSize, offset });
          if (queryGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tab.id) return;
          const historyEntry: QueryHistoryEntry | null = offset === 0 ? { id: `history-${Date.now()}-${Math.random().toString(16).slice(2)}`, connectionId, name: tab.name, language: tab.language, query, executedAt: Date.now() } : null;
          set((current) => ({
            result: { ...result, offset: result.offset ?? offset, limit: result.limit ?? current.pageSize }, resultRevision: current.resultRevision + 1,
            pageOffset: result.offset ?? offset, queryStatus: 'success',
            tabs: current.tabs.map((item) => item.id === tab.id ? { ...item, dirty: false, query } : item),
            queryHistory: historyEntry ? [historyEntry, ...current.queryHistory].slice(0, MAX_HISTORY_ENTRIES) : current.queryHistory,
          }));
          persist();
        } catch (error) {
          if (queryGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tab.id) return;
          set({ result: null, pageOffset: offset, queryStatus: 'error', queryError: asMessage(error) });
        }
      },
      async nextPage() { const next = get().result?.nextOffset; if (next !== null && next !== undefined) await get().runQuery(undefined, next); },
      async previousPage() { if (get().pageOffset > 0) await get().runQuery(undefined, Math.max(0, get().pageOffset - get().pageSize)); },
      async setPageSize(pageSize) { set({ pageSize, pageOffset: 0 }); if (get().result) await get().runQuery(undefined, 0); },

      newTab(language) {
        const state = get(); const connection = state.connections.find((item) => item.id === state.activeConnectionId); if (!connection) return;
        const compatibleLanguage: QueryLanguage = connection.kind === 'mongodb' ? 'mql' : 'sql'; const starter = starterForKind(connection.kind); const id = nextTabId();
        const connectionTabs = state.tabs.filter((tab) => tab.connectionId === connection.id);
        const nextTab: QueryTab = { id, connectionId: connection.id, name: `Untitled ${connectionTabs.length + 1}`, language: language === compatibleLanguage ? language : compatibleLanguage, query: starter.query, dirty: false };
        queryGeneration += 1; mutationGeneration += 1;
        set({ tabs: [...state.tabs, nextTab], activeTabId: id, result: null, pageOffset: 0, queryStatus: 'idle', queryError: null, mutations: [], changesOpen: false }); persist();
      },
      setTabLanguage(language) {
        const state = get(); const connection = state.connections.find((item) => item.id === state.activeConnectionId); if (!connection) return;
        const compatibleLanguage: QueryLanguage = connection.kind === 'mongodb' ? 'mql' : 'sql';
        if (language !== compatibleLanguage) return;
        set((current) => ({ tabs: current.tabs.map((tab) => tab.id === current.activeTabId ? { ...tab, language, dirty: true } : tab) })); persist();
      },
      closeTab(id) {
        const state = get(); const target = state.tabs.find((tab) => tab.id === id); if (!target) return;
        const connectionTabs = state.tabs.filter((tab) => tab.connectionId === target.connectionId); if (connectionTabs.length === 1) return;
        const tabs = state.tabs.filter((tab) => tab.id !== id); if (state.activeTabId !== id) { set({ tabs }); persist(); return; }
        const next = tabs.filter((tab) => tab.connectionId === target.connectionId).at(-1)!; queryGeneration += 1; mutationGeneration += 1;
        set({ tabs, activeTabId: next.id, result: null, pageOffset: 0, queryStatus: 'idle', queryError: null, mutations: [], changesOpen: false }); persist();
      },
      setActiveTab(activeTabId) {
        const state = get(); const tab = state.tabs.find((item) => item.id === activeTabId);
        if (!tab || tab.connectionId !== state.activeConnectionId || activeTabId === state.activeTabId) return;
        queryGeneration += 1; mutationGeneration += 1;
        set({ activeTabId, result: null, pageOffset: 0, queryStatus: 'idle', queryError: null, mutations: [], mutationStatus: 'idle', changesOpen: false });
      },
      restoreHistory(id) {
        const state = get();
        const entry = state.queryHistory.find((item) => item.id === id);
        const connection = state.connections.find((item) => item.id === state.activeConnectionId);
        const compatibleLanguage: QueryLanguage | undefined = connection ? (connection.kind === 'mongodb' ? 'mql' : 'sql') : undefined;
        if (!entry || entry.connectionId !== state.activeConnectionId || entry.language !== compatibleLanguage) return;
        queryGeneration += 1; mutationGeneration += 1;
        set((current) => ({
          tabs: current.tabs.map((tab) => tab.id === current.activeTabId ? { ...tab, query: entry.query, language: entry.language, name: entry.name, dirty: true } : tab),
          result: null, pageOffset: 0, queryStatus: 'idle', queryError: null,
          mutations: [], mutationStatus: 'idle', changesOpen: false,
        }));
        persist();
      },
      clearHistory(connectionId) { set((state) => ({ queryHistory: state.queryHistory.filter((entry) => entry.connectionId !== connectionId) })); persist(); },
      clearLocalWorkspace() {
        const disabled = setWorkspacePersistence(false);
        set({ queryHistory: [], localPersistenceEnabled: false, localPersistenceStatus: disabled.ok ? 'disabled' : 'error', localPersistenceMessage: disabled.message ?? null });
      },
      enableLocalWorkspace() {
        if (bridge.mode !== 'desktop') return;
        const enabled = setWorkspacePersistence(true);
        if (!enabled.ok) {
          set({ localPersistenceEnabled: false, localPersistenceStatus: 'error', localPersistenceMessage: enabled.message ?? null });
          return;
        }
        set({ localPersistenceEnabled: true, localPersistenceStatus: 'saved', localPersistenceMessage: null });
        persist();
      },
      focusNavigatorSearch() { set((state) => ({ navigatorOpen: true, navigatorSearchRequest: state.navigatorSearchRequest + 1 })); },

      stageCell(rowIndex, column, value) {
        const state = get(); const row = state.result?.rows[rowIndex]; const source = state.result?.editSource;
        if (!row || !source || !state.activeConnectionId || column === source.primaryKey) return;
        const rowKey = String(row[source.primaryKey]); if (!rowKey) return;
        const mutationId = `${rowKey}-${column}`; const existing = state.mutations.find((item) => item.id === mutationId); const previousValue = existing?.previousValue ?? row[column];
        const mutations = [...state.mutations.filter((item) => item.id !== mutationId), { id: mutationId, connectionId: state.activeConnectionId, table: source.table, primaryKey: source.primaryKey, rowKey, column, previousValue, nextValue: value }];
        set({ mutations, changesOpen: true });
      },
      discardMutation(id) { set((state) => ({ mutations: state.mutations.filter((item) => item.id !== id) })); },
      discardAllMutations() { mutationGeneration += 1; set({ mutations: [], changesOpen: false, mutationStatus: 'idle' }); },
      async applyMutations() {
        const state = get(); const connectionId = state.activeConnectionId; const tabId = state.activeTabId; const generation = mutationGeneration;
        const mutations = state.mutations.filter((mutation) => mutation.connectionId === connectionId);
        if (!connectionId || !mutations.length || mutations.length !== state.mutations.length) return;
        set({ mutationStatus: 'loading' });
        try {
          const result = await bridge.applyMutations(mutations);
          if (mutationGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return;
          if (result.applied !== mutations.length) throw new Error(`Expected ${mutations.length} applied changes, but received ${result.applied}.`);
          const submittedMutations = new Set(mutations);
          set((current) => {
            const remainingMutations = current.mutations.filter((mutation) => !submittedMutations.has(mutation));
            return { mutationStatus: 'success', mutations: remainingMutations, changesOpen: remainingMutations.length > 0,
              result: current.result ? { ...current.result, rows: current.result.rows.map((row) => { const primaryKey = current.result?.editSource?.primaryKey; const changes = primaryKey ? mutations.filter((change) => change.rowKey === String(row[primaryKey])) : []; return changes.reduce((updated, change) => ({ ...updated, [change.column]: change.nextValue }), row); }) } : null,
              toasts: [...current.toasts, { id: toastId(), tone: 'success', title: 'Changes applied', detail: result.message }],
            };
          });
        } catch (error) {
          if (mutationGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return;
          set((current) => ({ mutationStatus: 'error', toasts: [...current.toasts, { id: toastId(), tone: 'error', title: 'Could not apply changes', detail: asMessage(error) }] }));
        }
      },

      async askAi(prompt) {
        const cleanPrompt = prompt.trim(); if (!cleanPrompt) return null;
        const state = get(); const connection = state.connections.find((item) => item.id === state.activeConnectionId); const tab = state.tabs.find((item) => item.id === state.activeTabId);
        const connectionId = connection?.id; const tabId = tab?.id; const generation = ++aiGeneration;
        set((current) => ({ aiStatus: 'loading', aiMessages: [...current.aiMessages, { role: 'user', content: cleanPrompt }] }));
        try {
          const response = await bridge.askAi({ prompt: cleanPrompt, connectionId, databaseKind: connection?.kind, language: tab?.language, query: tab?.query });
          if (aiGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return null;
          const generatedQuery = response.generatedLanguage && response.generatedLanguage !== tab?.language ? undefined : response.generatedQuery;
          set((current) => ({ aiStatus: 'success', aiMessages: [...current.aiMessages, { role: 'assistant', content: response.message, query: generatedQuery, language: response.generatedLanguage }] })); return response;
        } catch (error) {
          if (aiGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return null;
          set((current) => ({ aiStatus: 'error', aiMessages: [...current.aiMessages, { role: 'assistant', content: `I couldn’t complete that request: ${asMessage(error)}` }] })); return null;
        }
      },
      useGeneratedQuery(query) { get().updateQuery(query); set({ aiOpen: true }); },
      async addConnection(profile) {
        const previous = get().connections.find((item) => item.id === profile.id);
        if (previous) nextLifecycleRequest(profile.id);
        const kindChanged = Boolean(previous && previous.kind !== profile.kind);
        set((state) => ({
          connections: previous ? state.connections.map((item) => item.id === profile.id ? profile : item) : [...state.connections, profile],
          tabs: kindChanged ? state.tabs.filter((tab) => tab.connectionId !== profile.id) : state.tabs,
          queryHistory: kindChanged ? state.queryHistory.filter((entry) => entry.connectionId !== profile.id) : state.queryHistory,
          connectionModalOpen: false,
          connectionModalProfileId: null,
        }));
        if (kindChanged) persist();
        await get().setActiveConnection(profile.id);
      },
      notify(tone, title, detail) { set((state) => ({ toasts: [...state.toasts, { id: toastId(), tone, title, detail }] })); },
      setUi(values) { set(values); },
      dismissToast(id) { set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) })); },
    };
  });
};

export type WorkspaceStore = ReturnType<typeof createWorkspaceStore>;
