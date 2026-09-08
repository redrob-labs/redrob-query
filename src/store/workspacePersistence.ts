import type { ConnectionProfile, QueryHistoryEntry, QueryLanguage, QueryTab } from '../domain/types';

export const WORKSPACE_STORAGE_KEY = 'redrob-data.workspace.v1';
export const WORKSPACE_PERSISTENCE_KEY = 'redrob-data.workspace-persistence.v1';
export const MAX_PERSISTED_TABS = 30;
export const MAX_HISTORY_ENTRIES = 50;
export const MAX_PERSISTED_QUERY_CHARACTERS = 200_000;
export const MAX_WORKSPACE_CHARACTERS = 1_500_000;

interface WorkspaceSnapshot {
  version: 1;
  tabs: QueryTab[];
  history: QueryHistoryEntry[];
}

export interface LoadedWorkspace extends Pick<WorkspaceSnapshot, 'tabs' | 'history'> {
  warning?: string;
  storageError?: string;
}

export interface WorkspaceSaveResult {
  ok: boolean;
  message?: string;
}

export interface WorkspacePersistencePreference extends WorkspaceSaveResult {
  enabled: boolean;
}

const expectedLanguage = (kind: ConnectionProfile['kind']): QueryLanguage => kind === 'mongodb' ? 'mql' : 'sql';
const cleanLabel = (value: unknown, maximum: number) => typeof value === 'string' ? value.trim().slice(0, maximum) : '';
const validDraftQuery = (value: unknown): value is string => typeof value === 'string' && value.length <= MAX_PERSISTED_QUERY_CHARACTERS;
const validHistoryQuery = (value: unknown): value is string => validDraftQuery(value) && value.length > 0;

export const loadWorkspacePersistencePreference = (): WorkspacePersistencePreference => {
  try {
    return { ok: true, enabled: localStorage.getItem(WORKSPACE_PERSISTENCE_KEY) !== 'disabled' };
  } catch {
    return { ok: false, enabled: false, message: 'Local query workspace storage is unavailable.' };
  }
};

export const clearWorkspace = (): WorkspaceSaveResult => {
  try {
    localStorage.removeItem(WORKSPACE_STORAGE_KEY);
    return { ok: true };
  } catch {
    return { ok: false, message: 'Local query workspace data could not be cleared.' };
  }
};

export const setWorkspacePersistence = (enabled: boolean): WorkspaceSaveResult => {
  try {
    localStorage.setItem(WORKSPACE_PERSISTENCE_KEY, enabled ? 'enabled' : 'disabled');
  } catch {
    return { ok: false, message: 'The local-saving preference could not be stored.' };
  }
  if (enabled) return { ok: true };
  const cleared = clearWorkspace();
  return cleared.ok ? cleared : { ok: false, message: `${cleared.message} Local saving remains disabled.` };
};

const clearedWarning = (description: string): Pick<LoadedWorkspace, 'warning' | 'storageError'> => {
  const cleared = clearWorkspace();
  return cleared.ok
    ? { warning: `${description} local query workspace data was cleared.` }
    : { warning: `${description} local query workspace data could not be cleared.`, storageError: cleared.message };
};

export const loadWorkspace = (profiles: ConnectionProfile[]): LoadedWorkspace => {
  const empty: LoadedWorkspace = { tabs: [], history: [] };
  const profilesById = new Map(profiles.map((profile) => [profile.id, profile]));
  let raw: string | null;
  try { raw = localStorage.getItem(WORKSPACE_STORAGE_KEY); } catch { return { ...empty, warning: 'Local query workspace storage is unavailable.', storageError: 'Local query workspace storage is unavailable.' }; }
  if (!raw) return empty;
  if (raw.length > MAX_WORKSPACE_CHARACTERS) return { ...empty, ...clearedWarning('Oversized') };

  try {
    const parsed = JSON.parse(raw) as Partial<WorkspaceSnapshot> | null;
    if (!parsed || parsed.version !== 1 || !Array.isArray(parsed.tabs) || !Array.isArray(parsed.history)) {
      return { ...empty, ...clearedWarning('Invalid') };
    }

    let discarded = false;
    const tabIds = new Set<string>();
    const tabs = parsed.tabs.flatMap((candidate) => {
      const profile = candidate && typeof candidate === 'object' ? profilesById.get(candidate.connectionId) : undefined;
      const id = candidate && typeof candidate === 'object' ? cleanLabel(candidate.id, 100) : '';
      if (!profile || !id || tabIds.has(id) || candidate.language !== expectedLanguage(profile.kind) || !validDraftQuery(candidate.query)) {
        discarded = true;
        return [];
      }
      tabIds.add(id);
      return [{ id, connectionId: profile.id, name: cleanLabel(candidate.name, 100) || 'Restored query', language: candidate.language, query: candidate.query, dirty: Boolean(candidate.dirty) }];
    }).slice(-MAX_PERSISTED_TABS);

    const historyIds = new Set<string>();
    const history = parsed.history.flatMap((candidate) => {
      const profile = candidate && typeof candidate === 'object' ? profilesById.get(candidate.connectionId) : undefined;
      const fallbackId = `history-${Number(candidate?.executedAt) || 0}`;
      const id = candidate && typeof candidate === 'object' ? cleanLabel(candidate.id, 100) || fallbackId : '';
      if (!profile || !id || historyIds.has(id) || candidate.language !== expectedLanguage(profile.kind) || !validHistoryQuery(candidate.query)) {
        discarded = true;
        return [];
      }
      historyIds.add(id);
      return [{
        id,
        connectionId: profile.id,
        name: cleanLabel(candidate.name, 100) || 'Query',
        language: candidate.language,
        query: candidate.query,
        executedAt: Number.isFinite(Number(candidate.executedAt)) ? Number(candidate.executedAt) : 0,
      }];
    }).slice(0, MAX_HISTORY_ENTRIES);

    return { tabs, history, ...(discarded ? { warning: 'Some invalid local query workspace entries were not restored.' } : {}) };
  } catch {
    return { ...empty, ...clearedWarning('Unreadable') };
  }
};

export const saveWorkspace = (tabs: QueryTab[], history: QueryHistoryEntry[]): WorkspaceSaveResult => {
  if (tabs.some((entry) => !validDraftQuery(entry.query)) || history.some((entry) => !validHistoryQuery(entry.query))) {
    const cleared = clearWorkspace();
    return { ok: false, message: cleared.ok
      ? `A tab draft or history query is invalid or exceeds the ${MAX_PERSISTED_QUERY_CHARACTERS.toLocaleString()} character local-save limit. Stored workspace data was cleared; the current workspace remains in memory only.`
      : `A tab draft or history query is invalid or exceeds the ${MAX_PERSISTED_QUERY_CHARACTERS.toLocaleString()} character local-save limit, and older local data could not be cleared.` };
  }
  const snapshot: WorkspaceSnapshot = {
    version: 1,
    tabs: tabs.slice(-MAX_PERSISTED_TABS).map(({ id, connectionId, name, language, query, dirty }) => ({ id, connectionId, name, language, query, dirty })),
    history: history.slice(0, MAX_HISTORY_ENTRIES).map(({ id, connectionId, name, language, query, executedAt }) => ({ id, connectionId, name, language, query, executedAt })),
  };
  try {
    const serialized = JSON.stringify(snapshot);
    if (serialized.length > MAX_WORKSPACE_CHARACTERS) {
      const cleared = clearWorkspace();
      return { ok: false, message: cleared.ok
        ? 'The local query workspace exceeds its storage budget. Stored workspace data was cleared; the current workspace remains in memory only.'
        : 'The local query workspace exceeds its storage budget, and older local data could not be cleared.' };
    }
    localStorage.setItem(WORKSPACE_STORAGE_KEY, serialized);
    return { ok: true };
  } catch {
    return { ok: false, message: 'Local query workspace storage is unavailable or full.' };
  }
};
