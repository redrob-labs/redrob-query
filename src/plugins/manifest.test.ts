// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from 'vitest';

import { PluginManifestError, readManifest, SUPPORTED_MANIFEST_VERSIONS } from './manifest';

/** The real published ER diagram manifest, which is what this loader exists to read. */
const erDiagramManifest = {
  id: 'bks-er-diagram',
  name: 'ER Diagram',
  author: { name: 'Beekeeper Studio', url: 'https://beekeeperstudio.io' },
  description: 'Interactive ER Diagram viewer',
  version: '1.1.2',
  icon: 'account_tree',
  manifestVersion: 1,
  minAppVersion: '5.5.0',
  capabilities: {
    views: [{ id: 'main-view', name: 'ER Diagram', type: 'plain-tab', entry: 'dist/index.html' }],
    menu: [
      { command: 'showAllEntities', name: 'Open ER Diagram', view: 'main-view', placement: 'menubar.tools' },
      { command: 'showOneSchema', name: 'View ERD', view: 'main-view', placement: 'entity.schema.context' },
      { command: 'showOneTable', name: 'View ERD', view: 'main-view', placement: 'entity.table.context' },
    ],
  },
};

describe('readManifest', () => {
  it('reads the real ER diagram manifest', () => {
    const manifest = readManifest(erDiagramManifest);
    expect(manifest).toMatchObject({
      id: 'bks-er-diagram',
      name: 'ER Diagram',
      version: '1.1.2',
      manifestVersion: 1,
      minAppVersion: '5.5.0',
    });
    expect(manifest.views).toEqual([
      { id: 'main-view', name: 'ER Diagram', type: 'plain-tab', entry: 'dist/index.html' },
    ]);
    expect(manifest.menu).toHaveLength(3);
  });

  it('ignores fields it does not model rather than failing', () => {
    // `author` is in the real manifest and this host has no use for it. A loader that rejected
    // unknown fields would break on every manifest the upstream format grows.
    expect(() => readManifest(erDiagramManifest)).not.toThrow();
  });

  it('refuses an entry that escapes the plugin directory', () => {
    for (const entry of ['../../etc/passwd', 'a/../../b.html', '..']) {
      const manifest = { ...erDiagramManifest, capabilities: { views: [{ id: 'v', name: 'V', entry }] } };
      expect(() => readManifest(manifest), entry).toThrow(/must not leave the plugin directory/);
    }
  });

  it('refuses an absolute entry', () => {
    const manifest = { ...erDiagramManifest, capabilities: { views: [{ id: 'v', name: 'V', entry: '/index.html' }] } };
    expect(() => readManifest(manifest)).toThrow(/must be relative to the plugin/);
  });

  it('refuses an entry that is a URL, which would load a remote page inside the app', () => {
    for (const entry of ['https://example.com/x.html', 'javascript:alert(1)', '//example.com/x.html', 'data:text/html,x']) {
      const manifest = { ...erDiagramManifest, capabilities: { views: [{ id: 'v', name: 'V', entry }] } };
      expect(() => readManifest(manifest), entry).toThrow(/must be a relative path, not a URL/);
    }
  });

  it('refuses an unsupported manifest version instead of guessing', () => {
    const manifest = { ...erDiagramManifest, manifestVersion: 99 };
    expect(() => readManifest(manifest)).toThrow(/manifestVersion 99 is not supported/);
    expect(SUPPORTED_MANIFEST_VERSIONS).toEqual([1]);
  });

  it('refuses duplicate view ids, which would share one state slot', () => {
    const manifest = {
      ...erDiagramManifest,
      capabilities: {
        views: [
          { id: 'main-view', name: 'A', entry: 'a.html' },
          { id: 'main-view', name: 'B', entry: 'b.html' },
        ],
      },
    };
    expect(() => readManifest(manifest)).toThrow(/duplicate view id main-view/);
  });

  it('refuses a menu item pointing at a view that does not exist', () => {
    const manifest = {
      ...erDiagramManifest,
      capabilities: {
        views: [{ id: 'main-view', name: 'V', entry: 'a.html' }],
        menu: [{ command: 'c', name: 'N', view: 'no-such-view', placement: 'menubar.tools' }],
      },
    };
    expect(() => readManifest(manifest)).toThrow(/targets unknown view no-such-view/);
  });

  it('refuses a plugin with no views, since there is nothing to mount', () => {
    const manifest = { ...erDiagramManifest, capabilities: { views: [] } };
    expect(() => readManifest(manifest)).toThrow(/declares no views/);
  });

  it('names the plugin in its errors', () => {
    const manifest = { ...erDiagramManifest, capabilities: { views: [{ id: 'v', name: 'V' }] } };
    expect(() => readManifest(manifest)).toThrow(/plugin bks-er-diagram view 0: entry/);
  });

  it('rejects non-objects and missing required strings', () => {
    for (const raw of [null, 'a string', 42, []]) {
      expect(() => readManifest(raw)).toThrow(PluginManifestError);
    }
    expect(() => readManifest({ manifestVersion: 1 })).toThrow(/id must be a non-empty string/);
    expect(() => readManifest({ id: 'x' })).toThrow(/manifestVersion must be an integer/);
    expect(() => readManifest({ id: 'x', manifestVersion: 1.5 })).toThrow(/manifestVersion must be an integer/);
  });

  it('defaults a view type but never an entry', () => {
    const manifest = {
      ...erDiagramManifest,
      capabilities: { views: [{ id: 'v', name: 'V', entry: 'a.html' }] },
    };
    expect(readManifest(manifest).views[0].type).toBe('plain-tab');
  });
});
