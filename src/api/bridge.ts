import { invoke } from '@tauri-apps/api/core';
import type {
  AiRequest,
  AiResponse,
  CellMutation,
  CellValue,
  ConnectionDraft,
  ConnectionProfile,
  ConnectionStatus,
  DatabaseKind,
  DataType,
  MetadataNode,
  MutationResult,
  QueryRequest,
  QueryResult,
} from '../domain/types';

export interface DataBridge {
  readonly mode: 'desktop' | 'demo';
  listConnections(): Promise<ConnectionProfile[]>;
  saveConnection(draft: ConnectionDraft): Promise<ConnectionProfile>;
  testConnection(draft: ConnectionDraft): Promise<ConnectionStatus>;
  loadMetadata(connectionId: string, parentId?: string | null): Promise<MetadataNode[]>;
  executeQuery(request: QueryRequest): Promise<QueryResult>;
  applyMutations(mutations: CellMutation[]): Promise<MutationResult>;
  saveAiKey(secret: string): Promise<void>;
  askAi(request: AiRequest): Promise<AiResponse>;
}

const wait = (ms = 180) => new Promise<void>((resolve) => setTimeout(resolve, ms));

const seededConnections: ConnectionProfile[] = [
  {
    id: 'demo-postgres', name: 'Acme Warehouse', kind: 'postgresql', host: 'demo.local', port: 5432,
    database: 'commerce', username: 'demo_user', tls: true, state: 'connected', isDemo: true,
  },
];

const postgresMetadata: MetadataNode[] = [
  { id: 'db-commerce', parentId: null, name: 'commerce', kind: 'database', childCount: 2 },
  { id: 'schema-public', parentId: 'db-commerce', name: 'public', kind: 'schema', childCount: 4 },
  { id: 'schema-analytics', parentId: 'db-commerce', name: 'analytics', kind: 'schema', childCount: 2 },
  { id: 'table-customers', parentId: 'schema-public', name: 'customers', kind: 'table', childCount: 7 },
  { id: 'table-orders', parentId: 'schema-public', name: 'orders', kind: 'table', childCount: 6 },
  { id: 'view-active', parentId: 'schema-public', name: 'active_customers', kind: 'view', childCount: 4 },
  { id: 'table-products', parentId: 'schema-public', name: 'products', kind: 'table', childCount: 5 },
  { id: 'view-revenue', parentId: 'schema-analytics', name: 'monthly_revenue', kind: 'view', childCount: 3 },
  { id: 'table-events', parentId: 'schema-analytics', name: 'events', kind: 'table', childCount: 4 },
  ...[
    ['id', 'uuid'], ['name', 'varchar'], ['email', 'varchar'], ['plan', 'varchar'],
    ['mrr', 'numeric'], ['active', 'boolean'], ['created_at', 'timestamptz'],
  ].map(([name, dataType]) => ({ id: `customers-${name}`, parentId: 'table-customers', name, kind: 'column' as const, dataType })),
  ...[
    ['id', 'uuid'], ['customer_id', 'uuid'], ['status', 'varchar'], ['total', 'numeric'],
    ['currency', 'char(3)'], ['created_at', 'timestamptz'],
  ].map(([name, dataType]) => ({ id: `orders-${name}`, parentId: 'table-orders', name, kind: 'column' as const, dataType })),
];

const metadataForProfile = (profile: ConnectionProfile): MetadataNode[] => {
  if (profile.id === 'demo-postgres') return postgresMetadata;
  const prefix = profile.id;
  const databaseName = profile.database || (profile.kind === 'sqlite' ? profile.filePath?.split('/').at(-1) || 'demo.sqlite' : 'sample');
  const databaseId = `${prefix}-database`;
  if (profile.kind === 'mongodb') {
    const collectionId = `${prefix}-customers`;
    return [
      { id: databaseId, parentId: null, name: databaseName, kind: 'database', childCount: 1 },
      { id: collectionId, parentId: databaseId, name: 'customers', kind: 'table', childCount: 6 },
      ...[
        ['_id', 'objectId'], ['name', 'string'], ['email', 'string'], ['tier', 'string'], ['active', 'boolean'], ['joinedAt', 'date'],
      ].map(([name, dataType]) => ({ id: `${collectionId}-${name}`, parentId: collectionId, name, kind: 'column' as const, dataType })),
    ];
  }
  const tableId = `${prefix}-customers`;
  return [
    { id: databaseId, parentId: null, name: databaseName, kind: 'database', childCount: 1 },
    { id: tableId, parentId: databaseId, name: 'customers', kind: 'table', childCount: profile.kind === 'sqlite' ? 5 : 7 },
    ...(profile.kind === 'sqlite'
      ? [['id', 'integer'], ['name', 'text'], ['email', 'text'], ['city', 'text'], ['created_at', 'text']]
      : [['id', 'varchar'], ['name', 'varchar'], ['email', 'varchar'], ['plan', 'varchar'], ['mrr', 'numeric'], ['active', 'boolean'], ['created_at', 'timestamp']]
    ).map(([name, dataType]) => ({ id: `${tableId}-${name}`, parentId: tableId, name, kind: 'column' as const, dataType })),
  ];
};

const names = ['Avery Stone', 'Mia Chen', 'Noah Williams', 'Sofia Patel', 'Leo Martin', 'Emma Garcia', 'Kai Johnson', 'Isla Brown', 'Mateo Silva', 'Zoe Kim'];
const plans = ['Scale', 'Growth', 'Starter'];
const seededCustomerRows = Array.from({ length: 64 }, (_, index) => {
  const sequence = index + 1;
  const name = names[index % names.length];
  return {
    id: `cus_${String(sequence).padStart(4, '0')}`,
    name: sequence <= names.length ? name : `${name} ${Math.floor(index / names.length) + 1}`,
    email: `${name.toLowerCase().replace(' ', '.').replace(/\s+/g, '')}${sequence}@example.com`,
    plan: plans[index % plans.length],
    mrr: [249, 599, 99, 799, 349][index % 5],
    active: index % 7 !== 0,
    created_at: `2025-${String((index % 8) + 1).padStart(2, '0')}-${String((index % 26) + 1).padStart(2, '0')}T10:30:00Z`,
  };
});

const seededMongoRows = seededCustomerRows.map((row) => ({
  _id: row.id.replace('cus_', '65f0a000000000000000'),
  name: row.name,
  email: row.email,
  tier: row.plan,
  active: row.active,
  joinedAt: row.created_at,
}));
const cities = ['Austin', 'Berlin', 'London', 'Singapore', 'Toronto'];
const seededSqliteRows = seededCustomerRows.map((row, index) => ({
  id: index + 1,
  name: row.name,
  email: row.email,
  city: cities[index % cities.length],
  created_at: row.created_at,
}));

const seededOrderRows = Array.from({ length: 96 }, (_, index) => ({
  id: `ord_${String(index + 1).padStart(5, '0')}`,
  customer_id: seededCustomerRows[index % seededCustomerRows.length].id,
  status: ['completed', 'completed', 'processing', 'refunded'][index % 4],
  total: [129.5, 799, 248.75, 64][index % 4],
  currency: 'USD',
  created_at: `2025-${String((index % 8) + 1).padStart(2, '0')}-${String((index % 26) + 1).padStart(2, '0')}T14:15:00Z`,
}));

const revenueRows = [
  { month: '2025-08', revenue: 128420, orders: 1842 },
  { month: '2025-07', revenue: 117890, orders: 1716 },
  { month: '2025-06', revenue: 109340, orders: 1628 },
  { month: '2025-05', revenue: 98420, orders: 1491 },
  { month: '2025-04', revenue: 91760, orders: 1407 },
  { month: '2025-03', revenue: 84610, orders: 1322 },
];

type RelationalDatabaseKind = Exclude<DatabaseKind, 'mongodb'>;
type RelationalFixture = {
  customerRows: Array<Record<string, CellValue>>;
  orderRows: Array<Record<string, CellValue>>;
};

const createRelationalFixture = (kind: RelationalDatabaseKind): RelationalFixture => ({
  customerRows: structuredClone(kind === 'sqlite' ? seededSqliteRows : seededCustomerRows),
  orderRows: structuredClone(seededOrderRows),
});

export class DemoBridge implements DataBridge {
  readonly mode = 'demo' as const;
  private connections = structuredClone(seededConnections);
  private relationalFixtures = new Map<string, RelationalFixture>([
    ['demo-postgres', createRelationalFixture('postgresql')],
  ]);

  async listConnections(): Promise<ConnectionProfile[]> {
    await wait(40);
    return structuredClone(this.connections);
  }

  async saveConnection(draft: ConnectionDraft): Promise<ConnectionProfile> {
    await wait();
    if (!draft.name.trim()) throw new Error('Connection name is required.');
    const { password: _password, ...profileDraft } = draft;
    const profile: ConnectionProfile = {
      ...profileDraft,
      id: `demo-${draft.kind}-${this.connections.length + 1}`,
      state: 'connected',
      isDemo: true,
    };
    if (profile.kind !== 'mongodb') {
      this.relationalFixtures.set(profile.id, createRelationalFixture(profile.kind));
    }
    this.connections.push(profile);
    return structuredClone(profile);
  }

  async testConnection(draft: ConnectionDraft): Promise<ConnectionStatus> {
    await wait(260);
    if (!draft.name.trim()) throw new Error('Add a connection name before testing.');
    if (draft.kind === 'sqlserver') return { ok: false, message: 'SQL Server is not available in this preview.' };
    if (draft.host.toLowerCase().includes('fail')) throw new Error('Demo test failed: host is unreachable.');
    return { ok: true, latencyMs: 28, message: 'Demo configuration looks valid' };
  }

  async loadMetadata(connectionId: string, parentId: string | null = null): Promise<MetadataNode[]> {
    await wait(110);
    const profile = this.connections.find((item) => item.id === connectionId);
    if (!profile) throw new Error('Connection not found.');
    return structuredClone(metadataForProfile(profile).filter((node) => node.parentId === parentId));
  }

  async executeQuery(request: QueryRequest): Promise<QueryResult> {
    await wait(340);
    const profile = this.connections.find((item) => item.id === request.connectionId);
    if (!profile) throw new Error('Connection not found.');
    const normalized = request.query.trim().toLowerCase();
    if (!normalized) throw new Error('Write a query before running it.');
    if (profile.kind === 'mongodb') {
      if (request.language !== 'mql') throw new Error('MongoDB connections accept MQL JSON, not SQL.');
      let operation: { collection?: string; operation?: string; filter?: Record<string, unknown>; limit?: number };
      try { operation = JSON.parse(request.query) as typeof operation; }
      catch { throw new Error('MongoDB demo queries must be valid JSON.'); }
      if (operation.operation !== 'find') throw new Error('MongoDB demo supports the read-only find operation.');
      if (operation.collection !== 'customers') throw new Error('MongoDB demo collection not found.');
      const filter = operation.filter ?? {};
      const rows = seededMongoRows.filter((row) => Object.entries(filter).every(([key, value]) => row[key as keyof typeof row] === value));
      const limit = Math.min(operation.limit ?? request.limit ?? 50, rows.length);
      return {
        columns: [
          { key: '_id', label: '_id', dataType: 'string', primaryKey: true },
          { key: 'name', label: 'name', dataType: 'string' },
          { key: 'email', label: 'email', dataType: 'string' },
          { key: 'tier', label: 'tier', dataType: 'string' },
          { key: 'active', label: 'active', dataType: 'boolean' },
          { key: 'joinedAt', label: 'joinedAt', dataType: 'date' },
        ], rows: structuredClone(rows.slice(0, limit)), rowCount: limit, durationMs: 31,
      };
    }
    if (request.language !== 'sql') throw new Error('Relational connections accept SQL, not MQL.');
    if (!normalized.startsWith('select') && !normalized.startsWith('with')) {
      throw new Error('The demo query runner accepts SELECT/CTE statements; use staged cell edits for changes.');
    }
    if (normalized.includes('syntax_error') || normalized.includes('missing_table')) {
      throw new Error('Query failed near “missing_table”. Check the table name and try again.');
    }
    const fixture = this.relationalFixtures.get(request.connectionId);
    if (!fixture) throw new Error('Demo relational fixture not found.');
    if (profile.kind === 'sqlite') {
      const limitMatch = normalized.match(/limit\s+(\d+)/);
      const limit = Math.min(Number(limitMatch?.[1] ?? request.limit ?? 50), fixture.customerRows.length);
      const rows = fixture.customerRows.slice(0, limit);
      return {
        columns: [
          { key: 'id', label: 'id', dataType: 'number', primaryKey: true },
          { key: 'name', label: 'name', dataType: 'string' },
          { key: 'email', label: 'email', dataType: 'string' },
          { key: 'city', label: 'city', dataType: 'string' },
          { key: 'created_at', label: 'created_at', dataType: 'date' },
        ], rows, rowCount: rows.length, durationMs: 24, editSource: { table: 'customers', primaryKey: 'id' },
      };
    }
    if (normalized.includes('sum(') || normalized.includes('revenue') || normalized.includes('group by')) {
      return {
        columns: [
          { key: 'month', label: 'month', dataType: 'string' },
          { key: 'revenue', label: 'revenue', dataType: 'number' },
          { key: 'orders', label: 'orders', dataType: 'number' },
        ], rows: revenueRows, rowCount: revenueRows.length, durationMs: 42,
      };
    }
    if (normalized.includes('from public.orders')) {
      const limitMatch = normalized.match(/limit\s+(\d+)/);
      const limit = Math.min(Number(limitMatch?.[1] ?? request.limit ?? 50), fixture.orderRows.length);
      const rows = fixture.orderRows.slice(0, limit);
      return {
        columns: [
          { key: 'id', label: 'id', dataType: 'string', primaryKey: true },
          { key: 'customer_id', label: 'customer_id', dataType: 'string' },
          { key: 'status', label: 'status', dataType: 'string' },
          { key: 'total', label: 'total', dataType: 'number' },
          { key: 'currency', label: 'currency', dataType: 'string' },
          { key: 'created_at', label: 'created_at', dataType: 'date' },
        ], rows, rowCount: rows.length, durationMs: 39, editSource: { table: 'public.orders', primaryKey: 'id' },
      };
    }
    const limitMatch = normalized.match(/limit\s+(\d+)/);
    const limit = Math.min(Number(limitMatch?.[1] ?? request.limit ?? 50), fixture.customerRows.length);
    const rows = normalized.includes("active = false") ? fixture.customerRows.filter((row) => !row.active).slice(0, limit) : fixture.customerRows.slice(0, limit);
    return {
      columns: [
        { key: 'id', label: 'id', dataType: 'string', primaryKey: true },
        { key: 'name', label: 'name', dataType: 'string' },
        { key: 'email', label: 'email', dataType: 'string' },
        { key: 'plan', label: 'plan', dataType: 'string' },
        { key: 'mrr', label: 'mrr', dataType: 'number' },
        { key: 'active', label: 'active', dataType: 'boolean' },
        { key: 'created_at', label: 'created_at', dataType: 'date' },
      ], rows, rowCount: rows.length, durationMs: 36, editSource: { table: profile.kind === 'postgresql' ? 'public.customers' : 'customers', primaryKey: 'id' },
    };
  }

  async applyMutations(mutations: CellMutation[]): Promise<MutationResult> {
    await wait(260);
    if (!mutations.length) throw new Error('There are no staged changes to apply.');
    let applied = 0;
    for (const mutation of mutations) {
      const profile = this.connections.find((item) => item.id === mutation.connectionId);
      if (!profile) throw new Error('Connection not found.');
      if (profile.kind === 'mongodb') throw new Error('MongoDB demo results are read-only.');
      const expectedTables = profile.kind === 'postgresql' ? ['public.customers', 'public.orders'] : ['customers'];
      if (!expectedTables.includes(mutation.table)) throw new Error('Mutation target does not belong to this demo connection.');
      const fixture = this.relationalFixtures.get(mutation.connectionId);
      if (!fixture) throw new Error('Demo relational fixture not found.');
      const sourceRows = mutation.table === 'public.orders' ? fixture.orderRows : fixture.customerRows;
      const row = sourceRows.find((item) => String(item[mutation.primaryKey]) === mutation.rowKey);
      if (row && mutation.column in row) {
        Object.assign(row, { [mutation.column]: mutation.nextValue });
        applied += 1;
      }
    }
    if (!applied) throw new Error('No matching demo rows were found.');
    return { applied, message: `${applied} change${applied === 1 ? '' : 's'} applied in demo memory` };
  }

  async saveAiKey(secret: string): Promise<void> {
    void secret;
    await wait(80);
  }

  async askAi(request: AiRequest): Promise<AiResponse> {
    await wait(420);
    const prompt = request.prompt.toLowerCase();
    if (request.databaseKind === 'mongodb' || request.language === 'mql') {
      if (prompt.includes('explain')) {
        return { intent: 'explanation', message: 'I can explain the active MongoDB JSON query. Result documents are not included in AI requests.' };
      }
      return {
        intent: 'query',
        message: prompt.includes('revenue')
          ? 'The MongoDB demo has no orders fixture, so I prepared the supported read-only customer find instead.'
          : 'I prepared a read-only customer find for the active MongoDB demo profile.',
        generatedLanguage: 'mql',
        generatedQuery: '{\n  "collection": "customers",\n  "operation": "find",\n  "filter": {},\n  "limit": 50\n}',
      };
    }
    if (prompt.includes('revenue') || prompt.includes('monthly')) {
      return {
        intent: 'query',
        message: 'I grouped completed orders by month and included order volume so you can compare growth at a glance.',
        generatedLanguage: 'sql',
        generatedQuery: "SELECT\n  TO_CHAR(created_at, 'YYYY-MM') AS month,\n  SUM(total) AS revenue,\n  COUNT(*) AS orders\nFROM public.orders\nWHERE status = 'completed'\nGROUP BY 1\nORDER BY 1 DESC\nLIMIT 12;",
      };
    }
    if (prompt.includes('explain') || prompt.includes('result')) {
      return {
        intent: 'explanation',
        message: 'I can explain the active query, but result rows are not included in AI requests. Share the relevant aggregate in your prompt if you want analysis of its values.',
      };
    }
    return { intent: 'guidance', message: 'I can write a query or explain the active query. Result rows and credentials are not included in AI requests.' };
  }
}

type WireKind = 'postgre_sql' | 'my_sql' | 's_q_lite' | 'mongo_db' | 'sql_server';
type WireValue = { type: string; value?: unknown };
interface WireProfile {
  id: string;
  name: string;
  kind: WireKind;
  config: { host?: string; port?: number; database?: string; username?: string; filePath?: string; srv?: boolean; tls: boolean; options: Record<string, string> };
  readOnly?: boolean;
  builtIn?: boolean;
}
interface WireMetadataNode {
  id: string; name: string; kind: string; dataType?: string; hasChildren: boolean;
}
interface WireResult {
  columns: Array<{ name: string; dataType: string; nullable: boolean | null }>;
  rows: WireValue[][];
  stats: { elapsedMs: number; rowsReturned: number };
}

const kindToWire: Record<DatabaseKind, WireKind> = {
  postgresql: 'postgre_sql', mysql: 'my_sql', sqlite: 's_q_lite', mongodb: 'mongo_db', sqlserver: 'sql_server',
};
const wireToKind: Record<WireKind, DatabaseKind> = {
  postgre_sql: 'postgresql', my_sql: 'mysql', s_q_lite: 'sqlite', mongo_db: 'mongodb', sql_server: 'sqlserver',
};
const toWireProfile = (draft: ConnectionDraft): Omit<WireProfile, 'id'> & { id?: string } => ({
  name: draft.name,
  kind: kindToWire[draft.kind],
  config: {
    ...(draft.host ? { host: draft.host } : {}),
    ...(draft.port ? { port: draft.port } : {}),
    ...(draft.database ? { database: draft.database } : {}),
    ...(draft.username ? { username: draft.username } : {}),
    ...(draft.filePath ? { filePath: draft.filePath } : {}),
    srv: Boolean(draft.srv),
    tls: draft.tls,
    options: draft.kind === 'mongodb' && draft.authSource ? { authSource: draft.authSource } : {},
  },
  readOnly: true,
  builtIn: false,
});
const fromWireProfile = (profile: WireProfile): ConnectionProfile => ({
  id: profile.id,
  name: profile.name,
  kind: wireToKind[profile.kind],
  host: profile.config.host ?? '',
  port: profile.config.port ?? 0,
  database: profile.config.database ?? '',
  username: profile.config.username ?? '',
  filePath: profile.config.filePath,
  srv: Boolean(profile.config.srv),
  tls: profile.config.tls,
  authSource: profile.kind === 'mongo_db' ? profile.config.options.authSource : undefined,
  state: 'disconnected',
  isDemo: Boolean(profile.builtIn),
});
const fromWireValue = (cell: WireValue): CellValue => {
  if (cell.type === 'null') return null;
  if (cell.type === 'boolean') return Boolean(cell.value);
  if (cell.type === 'integer') {
    const number = Number(cell.value);
    return Number.isSafeInteger(number) ? number : String(cell.value);
  }
  if (cell.type === 'float') {
    const number = Number(cell.value);
    return Number.isFinite(number) ? number : String(cell.value);
  }
  if (cell.type === 'decimal') return String(cell.value);
  if (cell.type === 'json' || cell.type === 'bson') return (cell.value ?? null) as CellValue;
  return String(cell.value ?? '');
};
const toDataType = (value: string): DataType => {
  const type = value.trim().toLowerCase();
  if (/^bit(?:\(\d+\))?$/.test(type)) return 'number';
  if (/int|decimal|numeric|float|double|real/.test(type)) return 'number';
  if (/^(?:bool|boolean)$/.test(type)) return 'boolean';
  if (/date|time/.test(type)) return 'date';
  if (/json|bson|object|array/.test(type)) return 'json';
  return 'string';
};
export class TauriBridge implements DataBridge {
  readonly mode = 'desktop' as const;
  private profiles = new Map<string, ConnectionProfile>();

  async listConnections(): Promise<ConnectionProfile[]> {
    const profiles = (await invoke<WireProfile[]>('list_connections')).map(fromWireProfile);
    profiles.forEach((profile) => this.profiles.set(profile.id, profile));
    return profiles;
  }

  async saveConnection(draft: ConnectionDraft): Promise<ConnectionProfile> {
    const wire = await invoke<WireProfile>('save_connection_with_secret', {
      profile: toWireProfile(draft),
      secret: draft.password?.trim() ? draft.password : null,
    });
    const profile = fromWireProfile(wire);
    this.profiles.set(profile.id, profile);
    return profile;
  }

  async testConnection(draft: ConnectionDraft): Promise<ConnectionStatus> {
    const status = await invoke<{ state: string; message: string; latencyMs?: number }>('test_connection', { profile: toWireProfile(draft), secret: draft.password || null });
    return { ok: status.state === 'connected', latencyMs: status.latencyMs, message: status.message };
  }

  async loadMetadata(connectionId: string, parentId: string | null = null): Promise<MetadataNode[]> {
    const nodes = await invoke<WireMetadataNode[]>('load_metadata', { request: { connectionId, parentId } });
    return nodes.map((node) => ({
      id: node.id, parentId, name: node.name,
      kind: node.kind === 'collection' ? 'table' : node.kind === 'server' ? 'database' : node.kind === 'index' ? 'column' : node.kind as MetadataNode['kind'],
      dataType: node.dataType,
      childCount: node.hasChildren ? 1 : 0,
    }));
  }

  async executeQuery(request: QueryRequest): Promise<QueryResult> {
    const page = await invoke<WireResult>('execute_query', { request: {
      connectionId: request.connectionId,
      query: request.query,
      language: request.language === 'mql' ? 'mongo_json' : 'sql',
      parameters: [], limit: request.limit ?? 500, offset: 0, timeoutMs: 30000,
    } });
    const keys = page.columns.map((column, index) => page.columns.findIndex((item) => item.name === column.name) === index ? column.name : `${column.name}_${index + 1}`);
    return {
      columns: page.columns.map((column, index) => ({ key: keys[index], label: column.name, dataType: toDataType(column.dataType), nullable: column.nullable ?? undefined })),
      rows: page.rows.map((row) => Object.fromEntries(keys.map((key, index) => [key, fromWireValue(row[index] ?? { type: 'null' })]))),
      rowCount: page.stats.rowsReturned,
      durationMs: page.stats.elapsedMs,
    };
  }

  async applyMutations(_mutations: CellMutation[]): Promise<MutationResult> {
    throw new Error('Desktop preview is read-only. Staged editing is available only in the browser demo.');
  }

  async saveAiKey(secret: string): Promise<void> {
    await invoke<void>('save_ai_secret', { secret });
  }

  async askAi(request: AiRequest): Promise<AiResponse> {
    const kind = request.databaseKind ?? (request.connectionId ? this.profiles.get(request.connectionId)?.kind : undefined) ?? 'postgresql';
    const language = request.language ?? (kind === 'mongodb' ? 'mql' : 'sql');
    const response = await invoke<{ message: { content: string } }>('ai_chat', { request: {
      prompt: request.prompt,
      ...(request.query === undefined ? {} : { activeQuery: request.query }),
      databaseKind: kindToWire[kind],
      queryLanguage: language === 'mql' ? 'mongo_json' : 'sql',
    } });
    const content = response.message.content;
    let generatedQuery: string | undefined;
    if (language === 'mql') {
      const fenced = content.match(/```(?:mql|json)\s*([\s\S]*?)```/i)?.[1]?.trim();
      if (fenced) {
        try {
          const candidate = JSON.parse(fenced) as { collection?: unknown; operation?: unknown };
          if (typeof candidate.collection === 'string' && typeof candidate.operation === 'string') generatedQuery = fenced;
        } catch { /* Ignore malformed generated JSON. */ }
      }
    } else {
      const fenced = content.match(/```sql\s*([\s\S]*?)```/i)?.[1]?.trim();
      const inline = content.match(/(?:SELECT|WITH)\s[\s\S]*?;/i)?.[0];
      const candidate = fenced ?? inline;
      if (candidate && /^(?:SELECT|WITH)\b/i.test(candidate)) generatedQuery = candidate;
    }
    return {
      message: content,
      generatedQuery,
      generatedLanguage: generatedQuery ? language : undefined,
      intent: generatedQuery ? 'query' : /explain|result/i.test(request.prompt) ? 'explanation' : 'guidance',
    };
  }
}

export const isTauriRuntime = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const createBridge = (): DataBridge => isTauriRuntime() ? new TauriBridge() : new DemoBridge();
