export type DatabaseKind = 'postgresql' | 'mysql' | 'sqlite' | 'mongodb' | 'sqlserver';
export type ConnectionState = 'connected' | 'disconnected' | 'testing' | 'error';
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
  authSource?: string;
  state: ConnectionState;
  isDemo?: boolean;
}

export interface ConnectionDraft extends Omit<ConnectionProfile, 'id' | 'state' | 'isDemo'> {
  password?: string;
}

export interface ConnectionStatus {
  ok: boolean;
  latencyMs?: number;
  message: string;
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
}

export interface ResultColumn {
  key: string;
  label: string;
  dataType: DataType;
  nullable?: boolean;
  primaryKey?: boolean;
}

export interface QueryResult {
  columns: ResultColumn[];
  rows: Record<string, CellValue>[];
  rowCount: number;
  durationMs: number;
  message?: string;
  editSource?: {
    table: string;
    primaryKey: string;
  };
}

export interface CellMutation {
  id: string;
  connectionId: string;
  table: string;
  primaryKey: string;
  rowKey: string;
  column: string;
  previousValue: CellValue;
  nextValue: CellValue;
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

export type AsyncStatus = 'idle' | 'loading' | 'success' | 'error';
export interface ToastMessage {
  id: string;
  tone: 'success' | 'error' | 'info';
  title: string;
  detail?: string;
}
