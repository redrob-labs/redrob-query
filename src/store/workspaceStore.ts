import { create, type StoreApi, type UseBoundStore } from 'zustand';
import type { DataBridge } from '../api/bridge';
import type {
  AiResponse,
  AsyncStatus,
  CellMutation,
  CellValue,
  ConnectionProfile,
  DatabaseKind,
  MetadataNode,
  QueryLanguage,
  QueryResult,
  QueryTab,
  ToastMessage,
} from '../domain/types';

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
LIMIT 50;`;

const sqliteStarter = `SELECT
  id,
  name,
  email,
  city,
  created_at
FROM customers
ORDER BY created_at DESC
LIMIT 50;`;

const mongoStarter = `{
  "collection": "customers",
  "operation": "find",
  "filter": {},
  "limit": 50
}`;

const starterForKind = (kind: DatabaseKind): { language: QueryLanguage; query: string; name: string } => {
  if (kind === 'mongodb') return { language: 'mql', query: mongoStarter, name: 'Customer documents' };
  if (kind === 'sqlite') return { language: 'sql', query: sqliteStarter, name: 'Customer overview' };
  if (kind === 'postgresql') return { language: 'sql', query: postgresStarter, name: 'Customer overview' };
  return { language: 'sql', query: 'SELECT *\nFROM customers\nLIMIT 50;', name: 'Customer overview' };
};

const makeStarterTab = (connection: ConnectionProfile, id: string): QueryTab => ({
  id,
  connectionId: connection.id,
  ...starterForKind(connection.kind),
  dirty: false,
});

interface AiMessage { role: 'user' | 'assistant'; content: string; query?: string; language?: QueryLanguage }

interface WorkspaceState {
  bridge: DataBridge;
  initialized: boolean;
  connections: ConnectionProfile[];
  activeConnectionId: string | null;
  metadata: Record<string, MetadataNode[]>;
  expandedNodes: Set<string>;
  tabs: QueryTab[];
  activeTabId: string;
  result: QueryResult | null;
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
  changesOpen: boolean;
  connectionModalOpen: boolean;
  commandPaletteOpen: boolean;
  toasts: ToastMessage[];
  initialize(): Promise<void>;
  loadNode(parentId?: string | null, force?: boolean): Promise<void>;
  toggleNode(node: MetadataNode): Promise<void>;
  setActiveConnection(id: string): Promise<void>;
  updateQuery(query: string): void;
  runQuery(queryOverride?: string): Promise<void>;
  newTab(language?: QueryLanguage): void;
  setTabLanguage(language: QueryLanguage): void;
  closeTab(id: string): void;
  setActiveTab(id: string): void;
  stageCell(rowIndex: number, column: string, value: CellValue): void;
  discardMutation(id: string): void;
  discardAllMutations(): void;
  applyMutations(): Promise<void>;
  askAi(prompt: string): Promise<AiResponse | null>;
  useGeneratedQuery(query: string): void;
  addConnection(profile: ConnectionProfile): Promise<void>;
  notify(tone: ToastMessage['tone'], title: string, detail?: string): void;
  setUi(values: Partial<Pick<WorkspaceState, 'aiOpen' | 'aiSettingsOpen' | 'aiWidth' | 'navigatorOpen' | 'changesOpen' | 'connectionModalOpen' | 'commandPaletteOpen'>>): void;
  dismissToast(id: string): void;
}

const asMessage = (error: unknown) => error instanceof Error ? error.message : 'Something went wrong.';
const toastId = () => `toast-${Date.now()}-${Math.random().toString(16).slice(2)}`;
const welcomeMessage = (mode: DataBridge['mode']): AiMessage => ({ role: 'assistant', content: mode === 'demo' ? 'I’m ready to explore the active demo data with you. Ask for a query or an explanation.' : 'I’m ready to help with the active connection. Ask for a query or an explanation.' });

export const createWorkspaceStore = (bridge: DataBridge): UseBoundStore<StoreApi<WorkspaceState>> => {
  let tabSequence = 0;
  let metadataGeneration = 0;
  let queryGeneration = 0;
  let mutationGeneration = 0;
  let aiGeneration = 0;
  const metadataRequests = new Map<string, number>();
  const nextTabId = () => `query-${++tabSequence}`;

  return create<WorkspaceState>((set, get) => ({
    bridge,
    initialized: false,
    connections: [],
    activeConnectionId: null,
    metadata: {},
    expandedNodes: new Set(),
    tabs: [],
    activeTabId: '',
    result: null,
    queryStatus: 'idle',
    queryError: null,
    mutations: [],
    mutationStatus: 'idle',
    aiMessages: [welcomeMessage(bridge.mode)],
    aiStatus: 'idle',
    aiOpen: true,
    aiSettingsOpen: false,
    aiWidth: 330,
    navigatorOpen: true,
    changesOpen: false,
    connectionModalOpen: false,
    commandPaletteOpen: false,
    toasts: [],

    async initialize() {
      if (get().initialized) return;
      try {
        const connections = await bridge.listConnections();
        const activeConnection = bridge.mode === 'demo' ? connections[0] : undefined;
        const starter = activeConnection ? makeStarterTab(activeConnection, nextTabId()) : null;
        set({
          connections,
          activeConnectionId: activeConnection?.id ?? null,
          tabs: starter ? [starter] : [],
          activeTabId: starter?.id ?? '',
          initialized: true,
        });
        if (activeConnection) await get().loadNode(null);
      } catch (error) {
        set({ initialized: true, queryError: asMessage(error), queryStatus: 'error' });
      }
    },

    async loadNode(parentId = null, force = false) {
      const connectionId = get().activeConnectionId;
      if (!connectionId) return;
      const generation = metadataGeneration;
      const key = parentId ?? 'root';
      if (!force && get().metadata[key]) return;
      const requestKey = `${connectionId}:${key}`;
      const requestToken = (metadataRequests.get(requestKey) ?? 0) + 1;
      metadataRequests.set(requestKey, requestToken);
      if (force) set((state) => {
        const metadata = { ...state.metadata };
        delete metadata[key];
        return { metadata };
      });
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
      else {
        expandedNodes.add(node.id);
        set({ expandedNodes });
        if (node.childCount) await get().loadNode(node.id);
        return;
      }
      set({ expandedNodes });
    },

    async setActiveConnection(id) {
      const connection = get().connections.find((item) => item.id === id);
      if (!connection) return;
      metadataGeneration += 1;
      queryGeneration += 1;
      mutationGeneration += 1;
      aiGeneration += 1;
      const existingTab = get().tabs.find((tab) => tab.connectionId === id);
      const tab = existingTab ?? makeStarterTab(connection, nextTabId());
      set((state) => ({
        activeConnectionId: id,
        metadata: {},
        expandedNodes: new Set(),
        tabs: existingTab ? state.tabs : [...state.tabs, tab],
        activeTabId: tab.id,
        result: null,
        queryStatus: 'idle',
        queryError: null,
        mutations: [],
        mutationStatus: 'idle',
        changesOpen: false,
        aiMessages: [welcomeMessage(bridge.mode)],
        aiStatus: 'idle',
      }));
      await get().loadNode(null);
    },

    updateQuery(query) {
      queryGeneration += 1;
      set((state) => ({
        tabs: state.tabs.map((tab) => tab.id === state.activeTabId ? { ...tab, query, dirty: true } : tab),
        ...(state.queryStatus === 'loading' ? { result: null, queryStatus: 'idle' as const, queryError: null } : {}),
      }));
    },

    async runQuery(queryOverride) {
      const state = get();
      const tab = state.tabs.find((item) => item.id === state.activeTabId);
      const connectionId = state.activeConnectionId;
      if (!tab || !connectionId || tab.connectionId !== connectionId) return;
      const query = queryOverride ?? tab.query;
      const generation = ++queryGeneration;
      mutationGeneration += 1;
      set({ queryStatus: 'loading', queryError: null, mutations: [], mutationStatus: 'idle', changesOpen: false });
      try {
        const result = await bridge.executeQuery({ connectionId, query, language: tab.language, limit: 100 });
        if (queryGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tab.id) return;
        set((current) => ({
          result,
          queryStatus: 'success',
          tabs: current.tabs.map((item) => item.id === tab.id ? { ...item, dirty: false, query } : item),
        }));
      } catch (error) {
        if (queryGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tab.id) return;
        set({ result: null, queryStatus: 'error', queryError: asMessage(error) });
      }
    },

    newTab(language) {
      const state = get();
      const connection = state.connections.find((item) => item.id === state.activeConnectionId);
      if (!connection) return;
      const compatibleLanguage: QueryLanguage = connection.kind === 'mongodb' ? 'mql' : 'sql';
      const starter = starterForKind(connection.kind);
      const id = nextTabId();
      const connectionTabs = state.tabs.filter((tab) => tab.connectionId === connection.id);
      const nextTab: QueryTab = {
        id,
        connectionId: connection.id,
        name: `Untitled ${connectionTabs.length + 1}`,
        language: language === compatibleLanguage ? language : compatibleLanguage,
        query: starter.query,
        dirty: false,
      };
      queryGeneration += 1;
      mutationGeneration += 1;
      set({ tabs: [...state.tabs, nextTab], activeTabId: id, result: null, queryStatus: 'idle', queryError: null, mutations: [], changesOpen: false });
    },

    setTabLanguage(language) {
      const state = get();
      const connection = state.connections.find((item) => item.id === state.activeConnectionId);
      if (!connection) return;
      const compatibleLanguage: QueryLanguage = connection.kind === 'mongodb' ? 'mql' : 'sql';
      if (language !== compatibleLanguage) {
        get().notify('info', 'Query language is fixed for this connection', connection.kind === 'mongodb' ? 'MongoDB uses MQL JSON.' : 'Relational connections use SQL.');
        return;
      }
      set((current) => ({ tabs: current.tabs.map((tab) => tab.id === current.activeTabId ? { ...tab, language, name: language === 'mql' ? 'Mongo query' : 'SQL query', dirty: true } : tab) }));
    },

    closeTab(id) {
      const state = get();
      const target = state.tabs.find((tab) => tab.id === id);
      if (!target) return;
      const connectionTabs = state.tabs.filter((tab) => tab.connectionId === target.connectionId);
      if (connectionTabs.length === 1) return;
      const tabs = state.tabs.filter((tab) => tab.id !== id);
      if (state.activeTabId !== id) { set({ tabs }); return; }
      const next = tabs.filter((tab) => tab.connectionId === target.connectionId).at(-1)!;
      queryGeneration += 1;
      mutationGeneration += 1;
      set({ tabs, activeTabId: next.id, result: null, queryStatus: 'idle', queryError: null, mutations: [], changesOpen: false });
    },

    setActiveTab(activeTabId) {
      const state = get();
      const tab = state.tabs.find((item) => item.id === activeTabId);
      if (!tab || tab.connectionId !== state.activeConnectionId || activeTabId === state.activeTabId) return;
      queryGeneration += 1;
      mutationGeneration += 1;
      set({ activeTabId, result: null, queryStatus: 'idle', queryError: null, mutations: [], mutationStatus: 'idle', changesOpen: false });
    },

    stageCell(rowIndex, column, value) {
      const state = get();
      const row = state.result?.rows[rowIndex];
      const source = state.result?.editSource;
      if (!row || !source || !state.activeConnectionId || column === source.primaryKey) return;
      const rowKey = String(row[source.primaryKey]);
      if (!rowKey) return;
      const mutationId = `${rowKey}-${column}`;
      const existing = state.mutations.find((item) => item.id === mutationId);
      const previousValue = existing?.previousValue ?? row[column];
      const mutations = [...state.mutations.filter((item) => item.id !== mutationId), {
        id: mutationId, connectionId: state.activeConnectionId, table: source.table, primaryKey: source.primaryKey, rowKey, column,
        previousValue, nextValue: value,
      }];
      set({ mutations, changesOpen: true });
    },

    discardMutation(id) { set((state) => ({ mutations: state.mutations.filter((item) => item.id !== id) })); },
    discardAllMutations() { mutationGeneration += 1; set({ mutations: [], changesOpen: false, mutationStatus: 'idle' }); },

    async applyMutations() {
      const state = get();
      const connectionId = state.activeConnectionId;
      const tabId = state.activeTabId;
      const generation = mutationGeneration;
      const mutations = state.mutations.filter((mutation) => mutation.connectionId === connectionId);
      if (!connectionId || !mutations.length || mutations.length !== state.mutations.length) return;
      set({ mutationStatus: 'loading' });
      try {
        const result = await bridge.applyMutations(mutations);
        if (mutationGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return;
        const submittedMutations = new Set(mutations);
        set((current) => {
          const remainingMutations = current.mutations.filter((mutation) => !submittedMutations.has(mutation));
          return {
          mutationStatus: 'success', mutations: remainingMutations, changesOpen: remainingMutations.length > 0,
          result: current.result ? { ...current.result, rows: current.result.rows.map((row) => {
            const primaryKey = current.result?.editSource?.primaryKey;
            const changes = primaryKey ? mutations.filter((change) => change.rowKey === String(row[primaryKey])) : [];
            return changes.reduce((updated, change) => ({ ...updated, [change.column]: change.nextValue }), row);
          }) } : null,
          toasts: [...current.toasts, { id: toastId(), tone: 'success', title: 'Changes applied', detail: result.message }],
          };
        });
      } catch (error) {
        if (mutationGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return;
        set((current) => ({ mutationStatus: 'error', toasts: [...current.toasts, { id: toastId(), tone: 'error', title: 'Could not apply changes', detail: asMessage(error) }] }));
      }
    },

    async askAi(prompt) {
      const cleanPrompt = prompt.trim();
      if (!cleanPrompt) return null;
      const state = get();
      const connection = state.connections.find((item) => item.id === state.activeConnectionId);
      const tab = state.tabs.find((item) => item.id === state.activeTabId);
      const connectionId = connection?.id;
      const tabId = tab?.id;
      const generation = ++aiGeneration;
      set((current) => ({ aiStatus: 'loading', aiMessages: [...current.aiMessages, { role: 'user', content: cleanPrompt }] }));
      try {
        const response = await bridge.askAi({
          prompt: cleanPrompt,
          connectionId,
          databaseKind: connection?.kind,
          language: tab?.language,
          query: tab?.query,
        });
        if (aiGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return null;
        const generatedQuery = response.generatedLanguage && response.generatedLanguage !== tab?.language ? undefined : response.generatedQuery;
        set((current) => ({ aiStatus: 'success', aiMessages: [...current.aiMessages, { role: 'assistant', content: response.message, query: generatedQuery, language: response.generatedLanguage }] }));
        return response;
      } catch (error) {
        if (aiGeneration !== generation || get().activeConnectionId !== connectionId || get().activeTabId !== tabId) return null;
        set((current) => ({ aiStatus: 'error', aiMessages: [...current.aiMessages, { role: 'assistant', content: `I couldn’t complete that request: ${asMessage(error)}` }] }));
        return null;
      }
    },

    useGeneratedQuery(query) {
      get().updateQuery(query);
      set({ aiOpen: true });
    },
    async addConnection(profile) {
      set((state) => ({ connections: [...state.connections, profile], connectionModalOpen: false }));
      await get().setActiveConnection(profile.id);
    },
    notify(tone, title, detail) { set((state) => ({ toasts: [...state.toasts, { id: toastId(), tone, title, detail }] })); },
    setUi(values) { set(values); },
    dismissToast(id) { set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) })); },
  }));
};

export type WorkspaceStore = ReturnType<typeof createWorkspaceStore>;
