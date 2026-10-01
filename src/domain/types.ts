export type DatabaseKind = 'postgresql' | 'mysql' | 'sqlite' | 'mongodb' | 'sqlserver';
export type ConnectionState = 'connected' | 'connecting' | 'disconnected' | 'testing' | 'error';
export type DataType = 'string' | 'number' | 'boolean' | 'date' | 'json' | 'null';
export type CellValue = string | number | boolean | null | Record<string, unknown>;
export type QueryLanguage = 'sql' | 'mql';

export interface ConnectionProfile {
  id: string;
  name: string;
  kind: DatabaseKind;
  host: string;
  port: number;
  database: string;
  username: string;
  filePath?: string;
  srv?: boolean;
  tls: boolean;
  /** Writes are refused unless this is explicitly false. Saved profiles default to read-only. */
  readOnly?: boolean;
  authSource?: string;
  state: ConnectionState;
  builtIn?: boolean;
  isDemo?: boolean;
}

export interface ConnectionDraft extends Omit<ConnectionProfile, 'id' | 'state' | 'isDemo' | 'builtIn'> {
  id?: string;
  password?: string;
}

export interface ConnectionStatus {
  ok: boolean;
  latencyMs?: number;
  message: string;
}

export interface SaveConnectionOutcome {
  profile: ConnectionProfile;
  warning?: string;
}

export interface RemoveConnectionOutcome {
  warning?: string;
}

export type MetadataKind = 'database' | 'schema' | 'table' | 'view' | 'column';
export interface MetadataNode {
  id: string;
  parentId: string | null;
  name: string;
  kind: MetadataKind;
  dataType?: string;
  childCount?: number;
}

export interface QueryRequest {
  connectionId: string;
  query: string;
  language: QueryLanguage;
  limit?: number;
  offset?: number;
}

export interface ResultColumn {
  key: string;
  label: string;
  dataType: DataType;
  nullable?: boolean;
  primaryKey?: boolean;
  /** The engine's value type for this column (desktop only), so an edit is written back as that type, not as text. */
  wireType?: string;
}

export interface QueryResult {
  columns: ResultColumn[];
  rows: Record<string, CellValue>[];
  rowCount: number;
  durationMs: number;
  offset?: number;
  limit?: number;
  nextOffset?: number | null;
  truncated?: boolean;
  message?: string;
  /** Why the result cannot be edited, when it cannot (desktop). */
  readOnlyReason?: string;
  editSource?: {
    schema?: string;
    table: string;
    primaryKey: string;
  };
}

export interface CellMutation {
  id: string;
  /** 'delete' removes the whole row and 'insert' adds one (column is empty for both); otherwise one cell changes. */
  kind?: 'update' | 'delete' | 'insert';
  /** For an insert: the columns filled in, with each column's engine type. Columns left out take the table default. */
  values?: { column: string; value: CellValue; wireType?: string }[];
  connectionId: string;
  table: string;
  primaryKey: string;
  rowKey: string;
  column: string;
  previousValue: CellValue;
  nextValue: CellValue;
  schema?: string;
  /** The key value as typed, and the engine types to write key and value back as. */
  keyValue?: CellValue;
  keyWireType?: string;
  wireType?: string;
}

export interface MutationResult {
  applied: number;
  message: string;
}

export interface AiRequest {
  prompt: string;
  connectionId?: string;
  databaseKind?: DatabaseKind;
  language?: QueryLanguage;
  query?: string;
}

export interface AiResponse {
  message: string;
  generatedQuery?: string;
  generatedLanguage?: QueryLanguage;
  intent: 'query' | 'explanation' | 'guidance';
}

export interface QueryTab {
  id: string;
  connectionId: string;
  name: string;
  language: QueryLanguage;
  query: string;
  dirty: boolean;
}

export interface QueryHistoryEntry {
  id: string;
  connectionId: string;
  name: string;
  language: QueryLanguage;
  query: string;
  executedAt: number;
}

export type AsyncStatus = 'idle' | 'loading' | 'success' | 'error';
export interface ToastMessage {
  id: string;
  tone: 'success' | 'error' | 'info';
  title: string;
  detail?: string;
}
