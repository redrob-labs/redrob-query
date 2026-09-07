import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { DemoBridge, type DataBridge } from './api/bridge';
import type { ConnectionProfile } from './domain/types';

async function renderWorkspace() {
  const user = userEvent.setup();
  render(<App bridge={new DemoBridge()} />);
  await screen.findByTestId('demo-banner');
  return user;
}

const desktopProfile: ConnectionProfile = {
  id: 'desktop-postgres', name: 'Local Postgres', kind: 'postgresql', host: 'localhost', port: 5432, database: 'app', username: 'reader', tls: true, state: 'disconnected',
};

const desktopBridge = (): DataBridge => ({
  mode: 'desktop',
  listConnections: vi.fn(async () => [desktopProfile]),
  saveConnection: vi.fn(),
  testConnection: vi.fn(),
  loadMetadata: vi.fn(async () => [{ id: 'desktop-db', parentId: null, name: 'desktop_app', kind: 'database' as const }]),
  executeQuery: vi.fn(),
  applyMutations: vi.fn(),
  saveAiKey: vi.fn(),
  askAi: vi.fn(),
});

describe('Redrob Data workspace', () => {
  it('waits for an explicit desktop profile selection before loading metadata', async () => {
    const user = userEvent.setup();
    const bridge = desktopBridge();
    render(<App bridge={bridge} />);

    await screen.findByTestId('desktop-banner');
    expect(bridge.listConnections).toHaveBeenCalledOnce();
    expect(bridge.loadMetadata).not.toHaveBeenCalled();
    expect(screen.getAllByText('No connection selected')).not.toHaveLength(0);
    expect(screen.getByText('Results read-only')).toBeInTheDocument();
    expect(screen.getByText('Waiting for connection selection')).toBeInTheDocument();
    expect(screen.getByText(/Database result rows and credentials are not automatically attached.*trusts user-entered content/i)).toBeInTheDocument();
    expect(screen.getByTestId('run-query')).toBeDisabled();
    expect(screen.queryByTestId('monaco-editor')).not.toBeInTheDocument();

    await user.click(screen.getByTestId('open-ai-settings-header'));
    expect(screen.getByText(/Desktop AI sends your prompt and optional active query to Redrob.*not automatically attached.*desktop renderer trusts user-entered content/i)).toBeInTheDocument();
    await user.click(screen.getByTestId('close-ai-settings'));

    await user.click(screen.getByRole('button', { name: 'Choose connection' }));
    await user.click(screen.getByRole('button', { name: 'Local Postgres' }));

    await waitFor(() => expect(bridge.loadMetadata).toHaveBeenCalledWith(desktopProfile.id, null));
    expect(await screen.findByText('desktop_app')).toBeInTheDocument();
    expect((screen.getByTestId('monaco-editor') as HTMLTextAreaElement).value).toContain('FROM public.customers');
    expect(screen.getByTestId('run-query')).toBeEnabled();
  });

  it('runs the starter query and renders a successful result state', async () => {
    const user = await renderWorkspace();
    expect(screen.getByText('Ready when you are')).toBeInTheDocument();
    await user.click(screen.getByTestId('run-query'));
    expect(screen.getByText('Executing against demo workspace…')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText(/50 rows in 36 ms/)).toBeInTheDocument());
    expect(screen.getByRole('columnheader', { name: /email/ })).toBeInTheDocument();
    expect(screen.queryByText('Query couldn’t run')).not.toBeInTheDocument();
    expect(screen.getByText('Writes require review')).toBeInTheDocument();
    expect(screen.queryByText('Read-only guard')).not.toBeInTheDocument();
  });

  it('tests and saves a connection from the connection manager', async () => {
    const user = await renderWorkspace();
    await user.click(screen.getByTestId('open-connection-modal'));
    expect(screen.getByTestId('connection-modal')).toBeInTheDocument();
    expect(screen.getAllByText('PostgreSQL')).toHaveLength(2);
    expect(screen.getByText('MySQL')).toBeInTheDocument();
    expect(screen.getByText('SQLite')).toBeInTheDocument();
    expect(screen.getByText('MongoDB')).toBeInTheDocument();
    expect(screen.getByText('SQL Server')).toBeInTheDocument();

    const name = screen.getByTestId('connection-name');
    await user.clear(name);
    await user.type(name, 'Finance replica');
    await user.click(screen.getByTestId('test-connection'));
    expect(await screen.findByText(/Demo configuration looks valid/)).toBeInTheDocument();
    await user.click(screen.getByTestId('save-connection'));
    await waitFor(() => expect(screen.queryByTestId('connection-modal')).not.toBeInTheDocument());
  });

  it('shows unsupported connection tests as errors without fake latency', async () => {
    const user = await renderWorkspace();
    await user.click(screen.getByTestId('open-connection-modal'));
    await user.click(screen.getByTestId('database-sqlserver'));
    expect(screen.getByTestId('save-connection')).toBeDisabled();
    await user.click(screen.getByTestId('test-connection'));
    const feedback = await screen.findByRole('status');
    expect(feedback).toHaveClass('error');
    expect(feedback).toHaveTextContent('not available in this preview');
    expect(feedback).not.toHaveTextContent('0 ms');
  });

  it('opens Redrob settings from both entry points and clears the key after demo save', async () => {
    const user = userEvent.setup();
    const bridge = new DemoBridge();
    const saveAiKey = vi.spyOn(bridge, 'saveAiKey');
    render(<App bridge={bridge} />);
    await screen.findByTestId('demo-banner');

    await user.click(screen.getByTestId('open-ai-settings-header'));
    expect(screen.getByRole('dialog', { name: 'Redrob settings' })).toBeInTheDocument();
    expect(screen.getByText(/operating system keychain/i)).toBeInTheDocument();
    expect(screen.getByText(/does not send credentials or store your key/i)).toBeInTheDocument();
    await user.click(screen.getByTestId('close-ai-settings'));

    await user.click(screen.getByTestId('open-ai-settings-rail'));
    await user.click(screen.getByTestId('save-ai-key'));
    expect(screen.getByRole('alert')).toHaveTextContent('Enter your Redrob API key');

    const secret = 'rrk_demo_secret_123';
    const input = screen.getByTestId('ai-key-input');
    await user.type(input, secret);
    await user.click(screen.getByTestId('save-ai-key'));

    expect(await screen.findByText(/Demo acknowledged/)).toBeInTheDocument();
    expect(saveAiKey).toHaveBeenCalledWith(secret);
    expect(input).toHaveValue('');
    expect(document.body).not.toHaveTextContent(secret);
  });

  it('generates a revenue query with AI and places it in the editor', async () => {
    const user = await renderWorkspace();
    const input = screen.getByTestId('ai-input');
    expect(screen.getByText(/Demo AI runs locally with your prompt and active query/i)).toBeInTheDocument();
    expect(screen.getByText(/Result rows and credentials are not used/i)).toBeInTheDocument();
    await user.type(input, 'Show monthly revenue');
    await user.click(screen.getByTestId('send-ai'));
    expect(await screen.findByText(/grouped completed orders by month/i)).toBeInTheDocument();
    expect(screen.getByText('Generated SQL')).toBeInTheDocument();
    await user.click(screen.getByTestId('use-ai-query'));
    await waitFor(() => expect((screen.getByTestId('monaco-editor') as HTMLTextAreaElement).value).toContain('SUM(total) AS revenue'));
  });

  it('saves controlled Mongo TLS/authSource fields and switches to MQL context', async () => {
    const user = userEvent.setup();
    const bridge = new DemoBridge();
    const saveConnection = vi.spyOn(bridge, 'saveConnection');
    render(<App bridge={bridge} />);
    await screen.findByTestId('demo-banner');
    await user.click(screen.getByTestId('open-connection-modal'));
    await user.click(screen.getByTestId('database-mongodb'));
    expect(screen.getByText('Authentication database')).toBeInTheDocument();
    expect(screen.queryByText('Database / auth source')).not.toBeInTheDocument();
    expect(screen.getByText('Require identity-verifying TLS')).toBeInTheDocument();
    expect(screen.getByText(/Private and self-signed CAs are not supported in this preview/i)).toBeInTheDocument();
    expect(screen.getByTestId('connection-tls')).toBeChecked();

    await user.clear(screen.getByTestId('connection-name'));
    await user.type(screen.getByTestId('connection-name'), 'Documents');
    await user.clear(screen.getByTestId('connection-database'));
    await user.type(screen.getByTestId('connection-database'), 'accounts');
    await user.clear(screen.getByTestId('connection-auth-source'));
    await user.type(screen.getByTestId('connection-auth-source'), 'admin');
    await user.click(screen.getByTestId('connection-tls'));
    await user.click(screen.getByTestId('save-connection'));

    await waitFor(() => expect(screen.queryByTestId('connection-modal')).not.toBeInTheDocument());
    expect(saveConnection).toHaveBeenCalledWith(expect.objectContaining({ kind: 'mongodb', database: 'accounts', authSource: 'admin', tls: false }));
    expect(screen.getByTestId('query-language')).toHaveTextContent('MQL');
    expect((screen.getByTestId('monaco-editor') as HTMLTextAreaElement).value).toContain('"operation": "find"');
    expect(screen.getAllByText('MO')).toHaveLength(2);
    expect(await screen.findByText('accounts')).toBeInTheDocument();
  });

  it('keeps relational tabs on SQL rather than creating an incompatible MQL tab', async () => {
    const user = await renderWorkspace();
    const language = screen.getByTestId('query-language');
    expect(language).toHaveTextContent('SQL');
    await user.click(language);
    expect(language).toHaveTextContent('SQL');
    expect(await screen.findByText('Query language is fixed for this connection')).toBeInTheDocument();
  });

  it('opens the command palette with the keyboard shortcut', async () => {
    const user = await renderWorkspace();
    await user.keyboard('{Meta>}k{/Meta}');
    expect(screen.getByTestId('command-palette')).toBeInTheDocument();
    expect(screen.getByText('Run current query')).toBeInTheDocument();
  });
});
