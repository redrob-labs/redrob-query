// SPDX-License-Identifier: GPL-3.0-or-later
//
// The host is driven the way a plugin drives it: by posting the real envelope and reading the real
// reply. Calling `dispatch` directly would test the switch statement and skip the two things most
// likely to be wrong -- the source check and the response shape.

import { beforeEach, describe, expect, it, vi } from 'vitest';

import { DemoBridge } from '../api/bridge';
import { PluginHost, type PluginHostContext } from './host';
import { KNOWN_UNIMPLEMENTED_METHODS } from './protocol';

/** A stand-in for the plugin's window. Only object identity matters to the source check. */
const pluginWindow = () => {
  const posted: unknown[] = [];
  return { posted, window: { postMessage: (message: unknown) => posted.push(message) } };
};

const mount = (overrides: Partial<PluginHostContext> = {}) => {
  const plugin = pluginWindow();
  const frame = { contentWindow: plugin.window } as unknown as HTMLIFrameElement;
  const state = new Map<string, string>();
  const titles: Array<[string, string]> = [];
  const context: PluginHostContext = {
    bridge: new DemoBridge(),
    activeConnectionId: () => 'demo-postgres',
    activeDialect: () => 'mysql' as const,
    appName: 'Redrob Query',
    appVersion: '0.1.0',
    readViewState: (viewId) => state.get(viewId) ?? null,
    writeViewState: (viewId, value) => void state.set(viewId, value),
    onTabTitle: (viewId, title) => void titles.push([viewId, title]),
    ...overrides,
  };
  const host = new PluginHost(frame, 'main-view', context);
  host.start();
  return { host, plugin, frame, state, titles };
};

/** Post a request as the plugin would, then wait for the reply. */
const ask = async (
  fixture: ReturnType<typeof mount>,
  name: string,
  args?: unknown,
  source?: unknown,
) => {
  const id = `req-${name}-${Math.random().toString(36).slice(2)}`;
  window.dispatchEvent(
    new MessageEvent('message', {
      data: { id, name, args },
      source: (source ?? fixture.plugin.window) as unknown as MessageEventSource,
    }),
  );
  // Generous, and for a measured reason: DemoBridge sleeps 180ms per loadMetadata call on purpose,
  // and the host walks the metadata tree level by level, so getTables with no schema costs four
  // sequential calls. A 250ms budget timed out on exactly the three traversal tests.
  for (let attempt = 0; attempt < 400; attempt += 1) {
    const reply = fixture.plugin.posted.find((m) => (m as { id?: string }).id === id);
    if (reply) return reply as { id: string; result?: unknown; error?: string };
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  return undefined;
};

describe('PluginHost', () => {
  let fixture: ReturnType<typeof mount>;

  beforeEach(() => {
    fixture = mount();
  });

  it('answers getAppInfo over the real envelope', async () => {
    const reply = await ask(fixture, 'getAppInfo');
    expect(reply).toMatchObject({ result: { name: 'Redrob Query', version: '0.1.0' } });
  });

  it('never puts name on a response', async () => {
    // The SDK's listener dispatches on `name` BEFORE it settles on `id`, so a response carrying a
    // name would resolve the promise and also fire every notification handler under that name.
    const reply = await ask(fixture, 'getAppInfo');
    expect(reply).toBeDefined();
    expect(reply).not.toHaveProperty('name');
  });

  it('puts name and never id on a notification', async () => {
    fixture.host.notify({ name: 'connectionChanged', args: { id: 'demo-postgres' } });
    const sent = fixture.plugin.posted.at(-1) as Record<string, unknown>;
    expect(sent).toMatchObject({ name: 'connectionChanged' });
    expect(sent).not.toHaveProperty('id');
  });

  it('reads schemas, tables and columns from the data layer', async () => {
    const schemas = await ask(fixture, 'getSchemas');
    expect(schemas?.result).toEqual([{ name: 'public' }, { name: 'analytics' }]);

    const tables = await ask(fixture, 'getTables', { schema: 'public' });
    expect(tables?.result).toEqual(
      expect.arrayContaining([
        { name: 'customers', schema: 'public', entityType: 'table' },
        { name: 'active_customers', schema: 'public', entityType: 'view' },
      ]),
    );

    const columns = await ask(fixture, 'getColumns', { table: 'customers', schema: 'public' });
    expect(columns?.result).toEqual(
      expect.arrayContaining([
        { columnName: 'id', dataType: 'uuid' },
        { columnName: 'mrr', dataType: 'numeric' },
      ]),
    );
  });

  it('lists tables across every schema when none is named', async () => {
    const tables = await ask(fixture, 'getTables');
    const schemas = new Set(
      (tables?.result as Array<{ schema?: string }>).map((table) => table.schema),
    );
    expect(schemas).toEqual(new Set(['public', 'analytics']));
  });

  it('round-trips view state so a plugin survives a reload', async () => {
    expect((await ask(fixture, 'getViewState'))?.result).toBeNull();
    await ask(fixture, 'setViewState', { state: '{"zoom":1.5}' });
    expect((await ask(fixture, 'getViewState'))?.result).toBe('{"zoom":1.5}');
  });

  it('reports a tab title against the view that asked', async () => {
    await ask(fixture, 'setTabTitle', { title: 'ER Diagram' });
    expect(fixture.titles).toEqual([['main-view', 'ER Diagram']]);
  });

  it('REFUSES the key methods on a dialect it cannot read, instead of answering with an empty list', async () => {
    // This is the assertion that matters most in this file. An ER diagram receiving [] for foreign
    // keys draws every table unconnected and reports nothing wrong -- it looks like a schema with no
    // relationships. A rejection is visible; an empty array is a lie.
    //
    // The refusal is BY DIALECT rather than by method: the SQL is ported and verified for SQLite and
    // PostgreSQL, and this fixture's connection is MySQL. "Keys are unsupported" and "keys are
    // unsupported on MySQL" are different facts, and the message says which one was hit.
    for (const method of ['getTableKeys', 'getIncomingKeys', 'getOutgoingKeys', 'getPrimaryKeys', 'getTableIndexes']) {
      const reply = await ask(fixture, method, { table: 'orders', schema: 'public' });
      expect(reply?.error, method).toContain('not implemented for mysql');
      expect(reply?.error, method).toContain('implemented: sqlite, postgresql');
      expect(reply, method).not.toHaveProperty('result');
    }
  });

  it('names every protocol method it knows but cannot answer', async () => {
    for (const method of KNOWN_UNIMPLEMENTED_METHODS) {
      const reply = await ask(fixture, method, {});
      expect(reply?.error, method).toContain(method);
    }
  });

  it('distinguishes an unknown method from an unimplemented one', async () => {
    const reply = await ask(fixture, 'definitelyNotAProtocolMethod');
    expect(reply?.error).toContain('unknown plugin request');
  });

  it('ignores a message from any window other than its own frame', async () => {
    const other = pluginWindow();
    const reply = await ask(fixture, 'getAppInfo', undefined, other.window);
    expect(reply).toBeUndefined();
    expect(fixture.plugin.posted).toEqual([]);
    expect(other.posted).toEqual([]);
  });

  it('ignores malformed envelopes rather than throwing', async () => {
    for (const data of [null, 'a string', 42, {}, { id: 'x' }, { name: 'getAppInfo' }, { id: '', name: 'getAppInfo' }]) {
      window.dispatchEvent(
        new MessageEvent('message', {
          data,
          source: fixture.plugin.window as unknown as MessageEventSource,
        }),
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(fixture.plugin.posted).toEqual([]);
  });

  it('reports a missing connection as an error, not as empty data', async () => {
    const detached = mount({ activeConnectionId: () => null });
    const reply = await ask(detached, 'getSchemas');
    expect(reply?.error).toContain('no active connection');
  });

  it('reports an unknown table rather than returning no columns', async () => {
    const reply = await ask(fixture, 'getColumns', { table: 'not_a_table', schema: 'public' });
    expect(reply?.error).toContain('table not found');
  });

  it('requires a table name for getColumns', async () => {
    const reply = await ask(fixture, 'getColumns', {});
    expect(reply?.error).toContain('requires a table name');
  });

  it('stops listening after stop()', async () => {
    fixture.host.stop();
    const reply = await ask(fixture, 'getAppInfo');
    expect(reply).toBeUndefined();
  });

  it('does not leak a listener when started twice', async () => {
    const spy = vi.spyOn(window, 'addEventListener');
    fixture.host.start();
    fixture.host.start();
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });
});

describe('PluginHost metadata cache', () => {
  /** A bridge that counts reads, so the claims below are measured rather than asserted. */
  const counting = () => {
    const real = new DemoBridge();
    const calls: Array<string | null | undefined> = [];
    let failNext = false;
    const bridge = {
      mode: real.mode,
      loadMetadata: (connectionId: string, parentId?: string | null) => {
        calls.push(parentId);
        if (failNext) {
          failNext = false;
          return Promise.reject(new Error('transient read failure'));
        }
        return real.loadMetadata(connectionId, parentId);
      },
    } as unknown as PluginHostContext['bridge'];
    return { bridge, calls, failSoon: () => void (failNext = true) };
  };

  it('reads each node once across many requests', async () => {
    const counter = counting();
    const fixture = mount({ bridge: counter.bridge });

    const tables = (await ask(fixture, 'getTables'))?.result as Array<{ name: string; schema?: string }>;
    const afterTables = counter.calls.length;
    for (const table of tables) {
      await ask(fixture, 'getColumns', { table: table.name, schema: table.schema });
    }

    // Measured before the cache existed: six tables cost 28 reads because every getColumns walked
    // the tree again. The tree is now read once and each table's columns exactly once.
    expect(afterTables).toBe(4);
    expect(counter.calls.length).toBe(afterTables + tables.length);
  });

  it('re-reads after invalidate, because a stale diagram is worse than a slow one', async () => {
    const counter = counting();
    const fixture = mount({ bridge: counter.bridge });

    await ask(fixture, 'getSchemas');
    const first = counter.calls.length;
    await ask(fixture, 'getSchemas');
    expect(counter.calls.length, 'served from cache').toBe(first);

    fixture.host.invalidate();
    await ask(fixture, 'getSchemas');
    expect(counter.calls.length).toBeGreaterThan(first);
  });

  it('does not remember a failed read as a result', async () => {
    const counter = counting();
    const fixture = mount({ bridge: counter.bridge });

    counter.failSoon();
    expect((await ask(fixture, 'getSchemas'))?.error).toContain('transient read failure');

    // Caching the rejected promise would poison this node for the host's whole lifetime.
    expect((await ask(fixture, 'getSchemas'))?.result).toEqual([{ name: 'public' }, { name: 'analytics' }]);
  });

  it('shares one round trip between concurrent requests for the same node', async () => {
    const counter = counting();
    const fixture = mount({ bridge: counter.bridge });

    await Promise.all([ask(fixture, 'getSchemas'), ask(fixture, 'getSchemas'), ask(fixture, 'getSchemas')]);
    // Caching the resolved value rather than the promise would let all three race and issue three.
    const rootReads = counter.calls.filter((parent) => parent === null || parent === undefined).length;
    expect(rootReads).toBe(1);
  });
});
