// SPDX-License-Identifier: GPL-3.0-or-later
//
// Mounts one plugin view in an iframe and binds a PluginHost to it.

import { useEffect, useRef, useState } from 'react';

import { PluginHost, type PluginHostContext } from './host';
import type { PluginManifest, PluginViewSpec } from './manifest';

/**
 * The iframe sandbox for third-party plugin content.
 *
 * `allow-scripts` alone, and the omission of `allow-same-origin` is the whole point rather than an
 * oversight. Plugin files are served from the application's own origin, so granting both would put
 * untrusted third-party HTML in the app's origin: it could then reach `window.parent` directly, read
 * the app's storage and cookies, and call into the DOM -- and the PluginHost's careful message
 * checking would be beside the point, because the plugin would not need messages at all.
 *
 * Without `allow-same-origin` the frame is given an opaque origin. postMessage still works in both
 * directions, which is the entire protocol, so nothing legitimate is lost. What IS lost is the
 * plugin's access to its own `localStorage` -- hence the protocol's getViewState/setViewState pair,
 * which the host answers from the app's storage on the plugin's behalf.
 *
 * Deliberately absent: `allow-popups`, `allow-modals`, `allow-top-navigation`, `allow-downloads`,
 * `allow-forms`. A schema viewer needs none of them, and `allow-top-navigation` would let a plugin
 * replace the whole application window.
 */
export const PLUGIN_SANDBOX = 'allow-scripts';

export interface PluginViewProps {
  manifest: PluginManifest;
  view: PluginViewSpec;
  /** Resolves a plugin-relative entry path to a loadable URL. */
  resolveEntry(manifest: PluginManifest, view: PluginViewSpec): string;
  context: PluginHostContext;
  onClose?(): void;
  /** Reported when the plugin asks for a tab title. */
  onTitle?(title: string): void;
}

export function PluginView({ manifest, view, resolveEntry, context, onClose, onTitle }: PluginViewProps) {
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const [title, setTitle] = useState(view.name);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame) return;
    const host = new PluginHost(frame, view.id, {
      ...context,
      onTabTitle: (viewId, next) => {
        setTitle(next);
        onTitle?.(next);
        context.onTabTitle?.(viewId, next);
      },
    });
    host.start();
    return () => host.stop();
    // The host is bound to this frame and view for the life of the mount. `context` is intentionally
    // not a dependency: it is a bag of callbacks that a parent re-creates on every render, and
    // including it would tear the host down and reload the plugin on each keystroke elsewhere in the
    // app. Connection changes are delivered by invalidate/notify, not by remounting.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view.id]);

  const source = resolveEntry(manifest, view);

  return (
    <section className="plugin-view" data-testid="plugin-workspace" data-state="mounted" aria-label={`${manifest.name}: ${title}`}>
      <header className="plugin-view-bar">
        <span className="plugin-view-title">{title}</span>
        <span className="plugin-view-origin">
          {manifest.name} {manifest.version}
        </span>
        {onClose ? (
          <button type="button" className="plugin-view-close" onClick={onClose} aria-label="Close plugin view">
            ×
          </button>
        ) : null}
      </header>
      <iframe
        ref={frameRef}
        className="plugin-view-frame"
        title={`${manifest.name}: ${title}`}
        src={source}
        sandbox={PLUGIN_SANDBOX}
        // A plugin has no reason to reach the network, and a schema viewer that tried to would be
        // exfiltrating the schema. This is a defence-in-depth hint rather than a guarantee, since it
        // does not restrict the frame's own scripts, so the sandbox above remains the real boundary.
        referrerPolicy="no-referrer"
      />
    </section>
  );
}
