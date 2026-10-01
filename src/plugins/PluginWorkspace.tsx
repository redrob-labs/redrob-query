// SPDX-License-Identifier: GPL-3.0-or-later
//
// Supplies a mounted plugin view with the host context the application can answer from.

import { Icon } from '../ui/Icon';
import { useCallback, useMemo, useRef } from 'react';

import type { DatabaseKind } from '../domain/types';

import { useWorkspace } from '../store/WorkspaceProvider';
import type { PluginHostContext } from './host';
import { PluginView } from './PluginView';
import {
  findPluginView,
  loadRegisteredPlugins,
  type RegisteredPlugin,
  resolvePluginEntry,
} from './registry';

/** Per-view plugin state, kept for the session. */
const VIEW_STATE_PREFIX = 'redrob-query.plugin-view-state.';

const readViewState = (viewId: string): string | null => {
  try {
    return window.localStorage.getItem(`${VIEW_STATE_PREFIX}${viewId}`);
  } catch {
    // Storage can be unavailable or full. A plugin that cannot restore its zoom level should still
    // open, so this is not an error the view needs to know about.
    return null;
  }
};

const writeViewState = (viewId: string, state: string): void => {
  try {
    window.localStorage.setItem(`${VIEW_STATE_PREFIX}${viewId}`, state);
  } catch {
    /* see readViewState */
  }
};

export interface PluginWorkspaceProps {
  /** `<pluginId>/<viewId>`, as the store holds it. */
  viewKey: string;
  onClose(): void;
  /**
   * Overrides the registry, for tests.
   *
   * The same seam `App` uses for its data bridge, and it exists for a specific reason: the
   * not-installed path must stay tested after the ER diagram's files were actually installed.
   * Asserting it through the real registry meant the guarantee evaporated the moment the flag
   * flipped, which is exactly when a regression would stop being caught.
   */
  plugins?: RegisteredPlugin[];
}

export function PluginWorkspace({ viewKey, onClose, plugins: injected }: PluginWorkspaceProps) {
  const bridge = useWorkspace((state) => state.bridge);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const connections = useWorkspace((state) => state.connections);
  const notify = useWorkspace((state) => state.notify);

  // The host reads the connection through a ref rather than closing over the value, so a connection
  // change does not need to remount the plugin and reload its whole page.
  const connectionRef = useRef(activeConnectionId);
  connectionRef.current = activeConnectionId;
  const dialectRef = useRef<DatabaseKind | null>(null);
  dialectRef.current = connections.find((profile) => profile.id === activeConnectionId)?.kind ?? null;

  const plugins = useMemo(
    () =>
      injected ??
      loadRegisteredPlugins((plugin, reason) =>
        notify('error', 'A plugin could not be loaded', `${plugin}: ${reason}`),
      ),
    [injected, notify],
  );
  const found = useMemo(() => findPluginView(plugins, viewKey), [plugins, viewKey]);
  const resolveEntry = useMemo(() => resolvePluginEntry(plugins), [plugins]);

  const context = useMemo<PluginHostContext>(
    () => ({
      bridge,
      activeConnectionId: () => connectionRef.current,
      activeDialect: () => dialectRef.current,
      appName: 'Redrob Query',
      appVersion: '0.1.0',
      readViewState,
      writeViewState,
    }),
    [bridge],
  );

  const onTitle = useCallback(() => {}, []);

  if (!found) {
    // Named rather than blank. A missing plugin is a registry or storage problem and the message has
    // to carry the key, because that is the only thing that identifies which view failed to open.
    return (
      <section className="plugin-view plugin-view-missing" data-testid="plugin-workspace" data-state="missing" role="alert">
        <p>
          This view is not available: <code>{viewKey}</code>
        </p>
        <button type="button" onClick={onClose}>
          <Icon name="arrowLeft" /> Back to the query workspace
        </button>
      </section>
    );
  }

  if (!found.plugin.installed) {
    // Deliberately NOT an iframe. This app answers an unmatched path with its own index.html, so a
    // frame pointing at an uninstalled plugin loads a nested copy of Redrob Query and presents it as
    // the plugin -- confidently wrong, which is worse than plainly empty.
    return (
      <section className="plugin-view plugin-view-missing" data-testid="plugin-workspace" data-state="not-installed">
        <p>
          <strong>{found.plugin.manifest.name}</strong> is registered but its files are not installed in
          this build.
        </p>
        <p>
          {found.plugin.manifest.description ?? 'This view has nothing to display yet.'}
        </p>
        <button type="button" onClick={onClose}>
          <Icon name="arrowLeft" /> Back to the query workspace
        </button>
      </section>
    );
  }

  return (
    <PluginView
      manifest={found.plugin.manifest}
      view={found.view}
      resolveEntry={resolveEntry}
      context={context}
      onClose={onClose}
      onTitle={onTitle}
    />
  );
}
