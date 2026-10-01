// SPDX-License-Identifier: GPL-3.0-or-later

import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { DemoBridge } from '../api/bridge';
import type { PluginHostContext } from './host';
import { readManifest } from './manifest';
import { PLUGIN_SANDBOX, PluginView } from './PluginView';

const manifest = readManifest({
  id: 'bks-er-diagram',
  name: 'ER Diagram',
  version: '1.1.2',
  manifestVersion: 1,
  capabilities: {
    views: [{ id: 'main-view', name: 'ER Diagram', type: 'plain-tab', entry: 'dist/index.html' }],
  },
});

const context = (): PluginHostContext => ({
  bridge: new DemoBridge(),
  activeConnectionId: () => 'demo-postgres',
  activeDialect: () => 'postgresql' as const,
  appName: 'Redrob Query',
  appVersion: '0.1.0',
  readViewState: () => null,
  writeViewState: () => {},
});

const mount = (overrides: Partial<Parameters<typeof PluginView>[0]> = {}) =>
  render(
    <PluginView
      manifest={manifest}
      view={manifest.views[0]}
      resolveEntry={(m, v) => `/plugins/${m.id}/${v.entry}`}
      context={context()}
      {...overrides}
    />,
  );

describe('PluginView', () => {
  it('mounts the view entry resolved against the plugin', () => {
    mount();
    const frame = screen.getByTitle('ER Diagram: ER Diagram') as HTMLIFrameElement;
    expect(frame.getAttribute('src')).toBe('/plugins/bks-er-diagram/dist/index.html');
  });

  it('sandboxes the frame with allow-scripts and WITHOUT allow-same-origin', () => {
    // The single most important assertion in this file. Plugin files are served from the app's own
    // origin, so granting allow-same-origin alongside allow-scripts would place untrusted
    // third-party HTML inside the app's origin -- it could then touch window.parent, storage and
    // cookies directly, and the host's message checks would be irrelevant because the plugin would
    // not need to send messages at all.
    mount();
    const frame = screen.getByTitle('ER Diagram: ER Diagram');
    const sandbox = frame.getAttribute('sandbox');
    expect(sandbox).toBe('allow-scripts');
    expect(PLUGIN_SANDBOX).toBe('allow-scripts');
    for (const forbidden of [
      'allow-same-origin',
      'allow-top-navigation',
      'allow-popups',
      'allow-modals',
      'allow-downloads',
      'allow-forms',
    ]) {
      expect(sandbox, forbidden).not.toContain(forbidden);
    }
  });

  it('shows which plugin and version is running, so untrusted content is attributed', () => {
    mount();
    expect(screen.getByText(/ER Diagram 1\.1\.2/)).toBeTruthy();
  });

  it('renames the header when the plugin sets a tab title', async () => {
    const onTitle = vi.fn();
    mount({ onTitle });
    const frame = screen.getByTitle('ER Diagram: ER Diagram') as HTMLIFrameElement;

    // The frame's contentWindow is the only source the host answers, so the request must claim it.
    window.dispatchEvent(
      new MessageEvent('message', {
        data: { id: 'r1', name: 'setTabTitle', args: { title: 'commerce · public' } },
        source: frame.contentWindow as MessageEventSource,
      }),
    );

    await waitFor(() => expect(onTitle).toHaveBeenCalledWith('commerce · public'));
    expect(screen.getByText('commerce · public')).toBeTruthy();
  });

  it('ignores a title request that does not come from its own frame', async () => {
    const onTitle = vi.fn();
    mount({ onTitle });
    window.dispatchEvent(
      new MessageEvent('message', {
        data: { id: 'r2', name: 'setTabTitle', args: { title: 'injected' } },
        source: window as unknown as MessageEventSource,
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(onTitle).not.toHaveBeenCalled();
  });

  it('stops listening when unmounted', async () => {
    const onTitle = vi.fn();
    const view = mount({ onTitle });
    const frame = screen.getByTitle('ER Diagram: ER Diagram') as HTMLIFrameElement;
    const source = frame.contentWindow as MessageEventSource;
    view.unmount();

    window.dispatchEvent(
      new MessageEvent('message', { data: { id: 'r3', name: 'setTabTitle', args: { title: 'late' } }, source }),
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(onTitle).not.toHaveBeenCalled();
  });

  it('offers a close affordance only when the parent can close it', () => {
    mount();
    expect(screen.queryByLabelText('Close plugin view')).toBeNull();
    mount({ onClose: () => {} });
    expect(screen.getByLabelText('Close plugin view')).toBeTruthy();
  });

  it('sends no referrer', () => {
    mount();
    expect(screen.getByTitle('ER Diagram: ER Diagram').getAttribute('referrerpolicy')).toBe('no-referrer');
  });
});
