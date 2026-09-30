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
    await screen.findByTestId('plugin-workspace');
    expect(screen.queryByTestId('query-editor')).toBeNull();
  });

  it('shows an honest empty state rather than a frame, while the plugin is not installed', async () => {
    // Found by checking the served build rather than assuming: this app answers an unmatched path
    // with its own index.html, so an iframe pointing at an uninstalled plugin loads a nested copy of
    // Redrob Query and presents it AS the ER diagram. Confidently wrong beats plainly empty is the
    // wrong way round, so no frame is mounted until the files are there.
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));

    const state = await screen.findByTestId('plugin-workspace');
    expect(state.dataset.state).toBe('not-installed');
    expect(state.textContent).toContain('ER Diagram');
    expect(state.textContent).toContain('not installed');
    expect(document.querySelector('iframe')).toBeNull();
  });

  it('closes from the rail and brings the query workspace back', async () => {
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));
    await screen.findByTestId('plugin-workspace');

    await user.click(screen.getByTestId('toggle-plugin-view'));
    await waitFor(() => expect(screen.queryByTestId('plugin-workspace')).toBeNull());
    expect(screen.getByTestId('query-editor')).toBeTruthy();
  });

  it('offers a way back from the empty state', async () => {
    const user = await openWorkspace();
    await user.click(screen.getByTestId('toggle-plugin-view'));
    await screen.findByTestId('plugin-workspace');

    await user.click(screen.getByText('Back to the query workspace'));
    await waitFor(() => expect(screen.queryByTestId('plugin-workspace')).toBeNull());
    expect(screen.getByTestId('query-editor')).toBeTruthy();
  });
});
