import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

import { DemoBridge, TauriBridge } from './bridge';

const request = (query: string, connectionId = 'demo-postgres') => ({ connectionId, query, language: 'sql' as const, limit: 50 });

describe('DemoBridge', () => {
  it('provides deterministic connections and expandable PostgreSQL metadata', async () => {
    const bridge = new DemoBridge();
    const connections = await bridge.listConnections();
    expect(connections).toEqual([expect.objectContaining({ id: 'demo-postgres', name: 'Acme Warehouse', isDemo: true, tls: true })]);

    const roots = await bridge.loadMetadata(connections[0].id);
    expect(roots).toEqual([expect.objectContaining({ id: 'db-commerce', name: 'commerce' })]);
    const schemas = await bridge.loadMetadata(connections[0].id, 'db-commerce');
    expect(schemas.map((node) => node.name)).toEqual(['public', 'analytics']);
    const tables = await bridge.loadMetadata(connections[0].id, 'schema-public');
    expect(tables.map((node) => node.name)).toContain('customers');
  });

  it('runs SELECT queries and returns typed rows', async () => {
    const bridge = new DemoBridge();
    const result = await bridge.executeQuery(request('SELECT * FROM public.customers LIMIT 3;'));
    expect(result.rows).toHaveLength(3);
    expect(result.columns).toContainEqual(expect.objectContaining({ key: 'mrr', dataType: 'number' }));
    expect(result.rows[0]).toEqual(expect.objectContaining({ id: 'cus_0001', plan: 'Scale' }));
  });

  it('saves Mongo profiles with Mongo metadata and an honest read-only find fixture', async () => {
    const bridge = new DemoBridge();
    const mongo = await bridge.saveConnection({
      name: 'Documents', kind: 'mongodb', host: 'localhost', port: 27017, database: 'accounts', username: 'reader', tls: true, authSource: 'admin',
    });
    expect((await bridge.listConnections()).at(-1)).toEqual(expect.objectContaining({ kind: 'mongodb', database: 'accounts', authSource: 'admin' }));
    const roots = await bridge.loadMetadata(mongo.id);
    expect(roots).toEqual([expect.objectContaining({ name: 'accounts' })]);
    expect(roots).not.toContainEqual(expect.objectContaining({ name: 'commerce' }));
    const collections = await bridge.loadMetadata(mongo.id, roots[0].id);
    expect(collections).toEqual([expect.objectContaining({ name: 'customers', kind: 'table' })]);
    const fields = await bridge.loadMetadata(mongo.id, collections[0].id);
    expect(fields.map((node) => node.name)).toContain('_id');

    const result = await bridge.executeQuery({
      connectionId: mongo.id,
      language: 'mql',
      query: '{"collection":"customers","operation":"find","filter":{"active":true},"limit":2}',
    });
    expect(result.rows).toHaveLength(2);
    expect(result.rows[0]).toEqual(expect.objectContaining({ _id: expect.any(String), tier: 'Growth', active: true }));
    expect(result.editSource).toBeUndefined();
    await expect(bridge.executeQuery({ connectionId: mongo.id, language: 'sql', query: 'SELECT * FROM customers' })).rejects.toThrow('MQL JSON');
    await expect(bridge.executeQuery({ connectionId: 'demo-postgres', language: 'mql', query: '{}' })).rejects.toThrow('accept SQL');
    const ai = await bridge.askAi({ prompt: 'Show monthly revenue', connectionId: mongo.id, databaseKind: 'mongodb', language: 'mql' });
    expect(ai.generatedLanguage).toBe('mql');
    expect(JSON.parse(ai.generatedQuery!)).toEqual(expect.objectContaining({ operation: 'find' }));
    expect(ai.generatedQuery).not.toContain('public.orders');
  });

  it('uses SQLite-specific metadata and starter-compatible rows', async () => {
    const bridge = new DemoBridge();
    const sqlite = await bridge.saveConnection({
      name: 'Local file', kind: 'sqlite', host: '', port: 0, database: '', username: '', filePath: '/tmp/demo.sqlite', tls: true,
    });
    const roots = await bridge.loadMetadata(sqlite.id);
    expect(roots[0].name).toBe('demo.sqlite');
    const result = await bridge.executeQuery({ connectionId: sqlite.id, language: 'sql', query: 'SELECT id, name, email, city, created_at FROM customers LIMIT 1;' });
    expect(result.rows[0]).toEqual(expect.objectContaining({ id: 1, city: 'Austin' }));
    expect(result.rows[0]).not.toHaveProperty('mrr');
  });

  it('rejects direct writes and supports deterministic AI revenue generation', async () => {
    const bridge = new DemoBridge();
    await expect(bridge.executeQuery(request('DELETE FROM customers;'))).rejects.toThrow('accepts SELECT/CTE');
    const response = await bridge.askAi({ prompt: 'Show monthly revenue', databaseKind: 'postgresql', language: 'sql' });
    expect(response.intent).toBe('query');
    expect(response.generatedLanguage).toBe('sql');
    expect(response.generatedQuery).toContain('SUM(total) AS revenue');
    const result = await bridge.executeQuery(request(response.generatedQuery!));
    expect(result.rows[0]).toEqual({ month: '2025-08', revenue: 128420, orders: 1842 });
  });

  it('acknowledges AI key saves without persisting or sending the secret', async () => {
    const bridge = new DemoBridge();
    const secret = 'rrk_demo_secret_123';
    await expect(bridge.saveAiKey(secret)).resolves.toBeUndefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(JSON.stringify(bridge)).not.toContain(secret);
  });

  it('isolates PostgreSQL customer and order edits from a same-engine peer saved afterward', async () => {
    const bridge = new DemoBridge();
    const result = await bridge.applyMutations([
      { id: 'cus_0001-plan', connectionId: 'demo-postgres', table: 'public.customers', primaryKey: 'id', rowKey: 'cus_0001', column: 'plan', previousValue: 'Scale', nextValue: 'Enterprise' },
      { id: 'ord_00001-status', connectionId: 'demo-postgres', table: 'public.orders', primaryKey: 'id', rowKey: 'ord_00001', column: 'status', previousValue: 'completed', nextValue: 'processing' },
    ]);
    expect(result).toEqual(expect.objectContaining({ applied: 2 }));

    const peer = await bridge.saveConnection({
      name: 'PostgreSQL peer', kind: 'postgresql', host: 'localhost', port: 5432, database: 'commerce_peer', username: 'demo', tls: true,
    });
    const [ownerCustomers, ownerOrders, peerCustomers, peerOrders] = await Promise.all([
      bridge.executeQuery(request('SELECT * FROM public.customers LIMIT 1;')),
      bridge.executeQuery(request('SELECT * FROM public.orders LIMIT 1;')),
      bridge.executeQuery(request('SELECT * FROM public.customers LIMIT 1;', peer.id)),
      bridge.executeQuery(request('SELECT * FROM public.orders LIMIT 1;', peer.id)),
    ]);
    expect(ownerCustomers.rows[0].plan).toBe('Enterprise');
    expect(ownerOrders.rows[0].status).toBe('processing');
    expect(peerCustomers.rows[0].plan).toBe('Scale');
    expect(peerOrders.rows[0].status).toBe('completed');

    await expect(bridge.applyMutations([{ id: 'cross', connectionId: 'demo-postgres', table: 'customers', primaryKey: 'id', rowKey: 'cus_0001', column: 'plan', previousValue: 'Enterprise', nextValue: 'Scale' }])).rejects.toThrow('does not belong');
  });

  it('isolates MySQL customer edits from a same-engine peer saved afterward', async () => {
    const bridge = new DemoBridge();
    const owner = await bridge.saveConnection({
      name: 'MySQL owner', kind: 'mysql', host: 'localhost', port: 3306, database: 'commerce', username: 'demo', tls: true,
    });
    await bridge.applyMutations([
      { id: 'mysql-plan', connectionId: owner.id, table: 'customers', primaryKey: 'id', rowKey: 'cus_0001', column: 'plan', previousValue: 'Scale', nextValue: 'MySQL Enterprise' },
    ]);

    const peer = await bridge.saveConnection({
      name: 'MySQL peer', kind: 'mysql', host: 'localhost', port: 3306, database: 'commerce_peer', username: 'demo', tls: true,
    });
    const [ownerCustomers, peerCustomers] = await Promise.all([
      bridge.executeQuery(request('SELECT * FROM customers LIMIT 1;', owner.id)),
      bridge.executeQuery(request('SELECT * FROM customers LIMIT 1;', peer.id)),
    ]);
    expect(ownerCustomers.rows[0].plan).toBe('MySQL Enterprise');
    expect(peerCustomers.rows[0].plan).toBe('Scale');
  });

  it('isolates SQLite customer edits from a same-engine peer saved afterward', async () => {
    const bridge = new DemoBridge();
    const owner = await bridge.saveConnection({
      name: 'SQLite owner', kind: 'sqlite', host: '', port: 0, database: '', username: '', filePath: '/tmp/owner.sqlite', tls: true,
    });
    await bridge.applyMutations([
      { id: 'sqlite-city', connectionId: owner.id, table: 'customers', primaryKey: 'id', rowKey: '1', column: 'city', previousValue: 'Austin', nextValue: 'Portland' },
    ]);

    const peer = await bridge.saveConnection({
      name: 'SQLite peer', kind: 'sqlite', host: '', port: 0, database: '', username: '', filePath: '/tmp/peer.sqlite', tls: true,
    });
    const [ownerCustomers, peerCustomers] = await Promise.all([
      bridge.executeQuery(request('SELECT * FROM customers LIMIT 1;', owner.id)),
      bridge.executeQuery(request('SELECT * FROM customers LIMIT 1;', peer.id)),
    ]);
    expect(ownerCustomers.rows[0]).toEqual(expect.objectContaining({ id: 1, city: 'Portland' }));
    expect(peerCustomers.rows[0]).toEqual(expect.objectContaining({ id: 1, city: 'Austin' }));
  });
});

describe('TauriBridge wire adapter', () => {
  beforeEach(() => invokeMock.mockReset());

  it('saves nested connection config and its secret through one atomic command', async () => {
    invokeMock.mockResolvedValueOnce({
      id: 'a8e5165c-a925-4f9c-b065-e395bbd433b4', name: 'Production', kind: 'postgre_sql',
      config: { host: 'db.internal', port: 5432, database: 'warehouse', username: 'analyst', tls: true, options: {} },
      readOnly: false, builtIn: false,
    });
    const bridge = new TauriBridge();
    const profile = await bridge.saveConnection({ name: 'Production', kind: 'postgresql', host: 'db.internal', port: 5432, database: 'warehouse', username: 'analyst', password: 'secret', tls: true });
    expect(profile).toEqual(expect.objectContaining({ state: 'disconnected', tls: true }));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('save_connection_with_secret', {
      profile: expect.objectContaining({ kind: 'postgre_sql', readOnly: true, config: expect.objectContaining({ host: 'db.internal', database: 'warehouse', tls: true, options: {} }) }),
      secret: 'secret',
    });
    expect(invokeMock.mock.calls[0][1].profile).not.toHaveProperty('password');
  });

  it('treats a whitespace-only password as no secret', async () => {
    invokeMock.mockResolvedValueOnce({
      id: 'sqlite-1', name: 'Local', kind: 's_q_lite',
      config: { filePath: '/tmp/local.sqlite', srv: false, tls: false, options: {} },
      readOnly: false, builtIn: false,
    });
    const bridge = new TauriBridge();
    await bridge.saveConnection({ name: 'Local', kind: 'sqlite', host: '', port: 0, database: '', username: '', filePath: '/tmp/local.sqlite', password: '   ', tls: false });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('save_connection_with_secret', expect.objectContaining({ secret: null }));
  });

  it('maps controlled TLS and separate Mongo authSource on save and hydration', async () => {
    invokeMock.mockResolvedValueOnce({
      id: 'mongo-1', name: 'Documents', kind: 'mongo_db',
      config: { host: 'mongo.internal', port: 27017, database: 'app', username: 'reader', srv: false, tls: false, options: { authSource: 'admin' } },
      readOnly: false, builtIn: false,
    });
    const bridge = new TauriBridge();
    const profile = await bridge.saveConnection({ name: 'Documents', kind: 'mongodb', host: 'mongo.internal', port: 27017, database: 'app', username: 'reader', tls: false, authSource: 'admin' });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('save_connection_with_secret', {
      profile: expect.objectContaining({ config: expect.objectContaining({ database: 'app', tls: false, options: { authSource: 'admin' } }) }),
      secret: null,
    });
    expect(profile).toEqual(expect.objectContaining({ kind: 'mongodb', database: 'app', authSource: 'admin', tls: false }));
  });

  it('maps a fulfilled unsupported connection status without inventing latency', async () => {
    invokeMock.mockResolvedValueOnce({ state: 'unsupported', message: 'SQL Server is not available in this preview.' });
    const bridge = new TauriBridge();
    const status = await bridge.testConnection({ name: 'Planned SQL Server', kind: 'sqlserver', host: 'localhost', port: 1433, database: 'master', username: 'sa', tls: true });
    expect(status).toEqual({ ok: false, latencyMs: undefined, message: 'SQL Server is not available in this preview.' });
  });

  it('rejects desktop mutations without invoking native commands', async () => {
    const bridge = new TauriBridge();
    await expect(bridge.applyMutations([{
      id: 'blocked-edit', connectionId: 'connection-1', table: 'customers', primaryKey: 'id',
      rowKey: 'row-1', column: 'email', previousValue: 'before@example.test', nextValue: 'after@example.test',
    }])).rejects.toThrow('Desktop preview is read-only');
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('saves the Redrob key through secure desktop storage', async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    const bridge = new TauriBridge();
    await bridge.saveAiKey('rrk_desktop_secret_123');
    expect(invokeMock).toHaveBeenCalledWith('save_ai_secret', { secret: 'rrk_desktop_secret_123' });
  });

  it('sends only the minimal SQL AI context and strips renderer-supplied extras', async () => {
    invokeMock.mockResolvedValueOnce({ message: { content: '```sql\nSELECT 1;\n```' } });
    const bridge = new TauriBridge();
    const rendererInput = {
      prompt: 'Write it', databaseKind: 'sqlite', language: 'sql', query: 'SELECT * FROM customers;',
      messages: [{ role: 'system', content: 'renderer-controlled' }],
      resultRows: [{ secret: 'row-must-not-cross-ipc' }],
      credentials: 'database-password-must-not-cross-ipc',
      model: 'renderer-model', baseUrl: 'https://renderer.invalid',
    } as Parameters<TauriBridge['askAi']>[0] & Record<string, unknown>;
    const response = await bridge.askAi(rendererInput);
    const requestBody = invokeMock.mock.calls[0][1].request;
    expect(invokeMock).toHaveBeenCalledWith('ai_chat', { request: {
      prompt: 'Write it',
      activeQuery: 'SELECT * FROM customers;',
      databaseKind: 's_q_lite',
      queryLanguage: 'sql',
    } });
    expect(requestBody).not.toHaveProperty('messages');
    expect(requestBody).not.toHaveProperty('resultRows');
    expect(requestBody).not.toHaveProperty('credentials');
    expect(requestBody).not.toHaveProperty('model');
    expect(requestBody).not.toHaveProperty('baseUrl');
    expect(JSON.stringify(requestBody)).not.toContain('row-must-not-cross-ipc');
    expect(JSON.stringify(requestBody)).not.toContain('database-password-must-not-cross-ipc');
    expect(response).toEqual(expect.objectContaining({ generatedQuery: 'SELECT 1;', generatedLanguage: 'sql' }));
  });

  it('requests Mongo JSON through the minimal contract and parses fenced mql/json without treating SQL as MQL', async () => {
    invokeMock.mockResolvedValueOnce({ message: { content: '```mql\n{"collection":"customers","operation":"find","filter":{}}\n```' } });
    const bridge = new TauriBridge();
    const response = await bridge.askAi({ prompt: 'Find customers', databaseKind: 'mongodb', language: 'mql', query: '{}' });
    expect(invokeMock).toHaveBeenCalledWith('ai_chat', { request: {
      prompt: 'Find customers', activeQuery: '{}', databaseKind: 'mongo_db', queryLanguage: 'mongo_json',
    } });
    expect(response.generatedLanguage).toBe('mql');
    expect(JSON.parse(response.generatedQuery!)).toEqual(expect.objectContaining({ operation: 'find' }));

    invokeMock.mockReset();
    invokeMock.mockResolvedValueOnce({ message: { content: '```sql\nSELECT * FROM customers;\n```' } });
    const mismatched = await bridge.askAi({ prompt: 'Find customers', databaseKind: 'mongodb', language: 'mql' });
    expect(invokeMock.mock.calls[0][1].request).toEqual({
      prompt: 'Find customers', databaseKind: 'mongo_db', queryLanguage: 'mongo_json',
    });
    expect(mismatched.generatedQuery).toBeUndefined();
  });

  it('maps every BIT value as numeric while BOOL and BOOLEAN stay boolean', async () => {
    invokeMock.mockResolvedValueOnce({
      columns: [
        { name: 'bit_zero', dataType: 'BIT', nullable: false },
        { name: 'bit_one', dataType: 'BIT(1)', nullable: false },
        { name: 'bit_two', dataType: 'BIT(2)', nullable: false },
        { name: 'bit_five', dataType: 'BIT(5)', nullable: false },
        { name: 'bool_value', dataType: 'BOOL', nullable: false },
        { name: 'boolean_value', dataType: 'BOOLEAN', nullable: false },
      ],
      rows: [[
        { type: 'integer', value: '0' },
        { type: 'integer', value: '1' },
        { type: 'integer', value: '2' },
        { type: 'integer', value: '5' },
        { type: 'boolean', value: false },
        { type: 'boolean', value: true },
      ]],
      stats: { elapsedMs: 4, rowsReturned: 1, rowsAffected: 0, truncated: false }, nextOffset: null,
    });
    const bridge = new TauriBridge();
    const result = await bridge.executeQuery({ connectionId: 'connection-1', query: 'SELECT bits', language: 'sql' });
    expect(result.columns.map((column) => column.dataType)).toEqual([
      'number', 'number', 'number', 'number', 'boolean', 'boolean',
    ]);
    expect(result.rows[0]).toEqual({
      bit_zero: 0,
      bit_one: 1,
      bit_two: 2,
      bit_five: 5,
      bool_value: false,
      boolean_value: true,
    });
  });

  it('keeps finite floats numeric while preserving decimals and unsafe integers exactly', async () => {
    invokeMock.mockResolvedValueOnce({
      columns: [
        { name: 'safe', dataType: 'integer', nullable: false },
        { name: 'unsafe', dataType: 'integer', nullable: false },
        { name: 'ratio', dataType: 'double', nullable: false },
        { name: 'amount', dataType: 'numeric', nullable: false },
        { name: 'invalid', dataType: 'double', nullable: false },
      ],
      rows: [[
        { type: 'integer', value: '42' },
        { type: 'integer', value: '9007199254740993' },
        { type: 'float', value: '1.25e-3' },
        { type: 'decimal', value: '42.50' },
        { type: 'float', value: 'NaN' },
      ]],
      stats: { elapsedMs: 17, rowsReturned: 1, rowsAffected: 0, truncated: false }, nextOffset: null,
    });
    const bridge = new TauriBridge();
    const result = await bridge.executeQuery({ connectionId: 'connection-1', query: 'SELECT values', language: 'sql' });
    expect(result.rows[0]).toEqual({ safe: 42, unsafe: '9007199254740993', ratio: 0.00125, amount: '42.50', invalid: 'NaN' });
  });
});
