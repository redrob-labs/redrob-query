// SPDX-License-Identifier: GPL-3.0-or-later
//
// The plugin view reached the way a user reaches it: by clicking the rail.
//
// The previous cycle left the host tested and unmounted, and the build was byte-identical because
// nothing imported it. These assertions are the difference -- they fail if the view is wired up
// anywhere short of the actual application shell.

import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { App } from '../App';
import { DemoBridge } from '../api/bridge';

const openWorkspace = async () => {
  const user = userEvent.setup();
  render(<App bridge={new DemoBridge()} />);
  await screen.findByTestId('demo-banner');
  return user;
};

describe('plugin view in the application shell', () => {
  it('is not mounted until it is asked for', async () => {
    await openWorkspace();
    expect(screen.queryByTestId('plugin-workspace')).toBeNull();
    expect(screen.getByTestId('query-editor')).toBeTruthy();
  });

  it('opens from the rail and replaces the query workspace', async () => {
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));

    // A plain-tab view owns the main area, so the editor and grid must be gone rather than merely
    // covered: leaving them mounted would keep a hidden result grid live behind the diagram.
    const view = await screen.findByTestId('plugin-workspace');
    expect(view.dataset.state).toBe('mounted');
    expect(screen.queryByTestId('query-editor')).toBeNull();
  });

  it('loads the installed ER diagram from the plugin base URL, sandboxed', async () => {
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));
    const frame = (await screen.findByTitle(/ER Diagram/)) as HTMLIFrameElement;
    expect(frame.getAttribute('src')).toBe('/plugins/bks-er-diagram/dist/index.html');
    expect(frame.getAttribute('sandbox')).toBe('allow-scripts');
  });

  it('closes from the rail and brings the query workspace back', async () => {
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));
    await screen.findByTestId('plugin-workspace');

    await user.click(screen.getByTestId('toggle-plugin-view'));
    await waitFor(() => expect(screen.queryByTestId('plugin-workspace')).toBeNull());
    expect(screen.getByTestId('query-editor')).toBeTruthy();
  });

  it('closes from the view header too', async () => {
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));
    await screen.findByTestId('plugin-workspace');

    await user.click(screen.getByLabelText('Close plugin view'));
    await waitFor(() => expect(screen.queryByTestId('plugin-workspace')).toBeNull());
  });

  it('answers a request from the mounted frame with real schema data', async () => {
    // The whole point of mounting: the host inside the shell reaches the application's own data.
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));
    const frame = (await screen.findByTitle(/ER Diagram/)) as HTMLIFrameElement;

    const replies: unknown[] = [];
    const target = frame.contentWindow as Window & typeof globalThis;
    const original = target.postMessage.bind(target);
    target.postMessage = ((message: unknown, ...rest: unknown[]) => {
      replies.push(message);
      return original(message as never, ...(rest as []));
    }) as typeof target.postMessage;

    window.dispatchEvent(
      new MessageEvent('message', {
        data: { id: 'shell-1', name: 'getTables', args: { schema: 'public' } },
        source: frame.contentWindow as MessageEventSource,
      }),
    );

    await waitFor(
      () => {
        const reply = replies.find((message) => (message as { id?: string }).id === 'shell-1');
        expect(reply).toBeDefined();
        expect((reply as { result?: unknown[] }).result).toEqual(
          expect.arrayContaining([{ name: 'customers', schema: 'public', entityType: 'table' }]),
        );
      },
      { timeout: 5000 },
    );
  });
});
