// SPDX-License-Identifier: GPL-3.0-or-later
//
// The not-installed and unknown-view paths, tested with an injected registry.
//
// These were in the shell test until the ER diagram's files were actually installed, at which point
// asserting them through the real registry started failing -- the guarantee would have evaporated at
// exactly the moment a regression stopped being caught. They are injected here so they hold whatever
// the real registry says.

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { DemoBridge } from '../api/bridge';
import { WorkspaceProvider } from '../store/WorkspaceProvider';
import { readManifest } from './manifest';
import { PluginWorkspace } from './PluginWorkspace';
import type { RegisteredPlugin } from './registry';

const manifest = readManifest({
  id: 'bks-er-diagram',
  name: 'ER Diagram',
  description: 'Interactive ER Diagram viewer for relational database schemas',
  version: '1.1.2',
  manifestVersion: 1,
  capabilities: {
    views: [{ id: 'main-view', name: 'ER Diagram', type: 'plain-tab', entry: 'dist/index.html' }],
  },
});

const plugin = (installed: boolean): RegisteredPlugin => ({
  manifest,
  baseUrl: '/plugins/bks-er-diagram/',
  installed,
});

const mount = (plugins: RegisteredPlugin[], viewKey = 'bks-er-diagram/main-view', onClose = vi.fn()) => {
  render(
    <WorkspaceProvider bridge={new DemoBridge()}>
      <PluginWorkspace viewKey={viewKey} onClose={onClose} plugins={plugins} />
    </WorkspaceProvider>,
  );
  return onClose;
};

describe('PluginWorkspace install state', () => {
  it('mounts a frame when the plugin is installed', () => {
    mount([plugin(true)]);
    const area = screen.getByTestId('plugin-workspace');
    expect(area.dataset.state).toBe('mounted');
    const frame = document.querySelector('iframe');
    expect(frame?.getAttribute('src')).toBe('/plugins/bks-er-diagram/dist/index.html');
  });

  it('mounts NO frame when the plugin is registered but not installed', () => {
    // The defect this guards, measured on a real served build: this app answers an unmatched path
    // with its own index.html, so a frame pointing at an uninstalled plugin loads a nested copy of
    // Redrob Query and presents it AS the ER diagram. A confident wrong picture is worse than an
    // honest empty one, and an HTTP probe cannot tell the two apart -- hence a build-time flag.
    mount([plugin(false)]);
    const area = screen.getByTestId('plugin-workspace');
    expect(area.dataset.state).toBe('not-installed');
    expect(area.textContent).toContain('ER Diagram');
    expect(area.textContent).toContain('not installed');
    expect(document.querySelector('iframe')).toBeNull();
  });

  it('names the plugin and its purpose in the empty state', () => {
    mount([plugin(false)]);
    expect(screen.getByText(/Interactive ER Diagram viewer/)).toBeTruthy();
  });

  it('offers a way back from the empty state', async () => {
    const user = userEvent.setup();
    const onClose = mount([plugin(false)]);
    await user.click(screen.getByText('Back to the query workspace'));
    expect(onClose).toHaveBeenCalled();
  });

  it('reports an unknown view key rather than rendering blank', () => {
    mount([plugin(true)], 'no-such-plugin/main-view');
    const area = screen.getByTestId('plugin-workspace');
    expect(area.dataset.state).toBe('missing');
    expect(area.textContent).toContain('no-such-plugin/main-view');
  });

  it('reports a known plugin asked for an unknown view', () => {
    mount([plugin(true)], 'bks-er-diagram/not-a-view');
    expect(screen.getByTestId('plugin-workspace').dataset.state).toBe('missing');
  });

  it('offers a way back from the unknown-view state too', async () => {
    const user = userEvent.setup();
    const onClose = mount([plugin(true)], 'bks-er-diagram/not-a-view');
    await user.click(screen.getByText('Back to the query workspace'));
    expect(onClose).toHaveBeenCalled();
  });
});
