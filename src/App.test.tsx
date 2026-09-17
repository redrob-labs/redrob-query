import { render, screen, waitFor, within } from '@testing-library/react';
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
  profileWarnings: vi.fn(async () => []),
  saveConnection: vi.fn(),
  removeConnection: vi.fn(async () => ({})),
  connect: vi.fn(async () => ({ ok: true, message: 'Connected' })),
  disconnect: vi.fn(),
  testConnection: vi.fn(),
  loadMetadata: vi.fn(async () => [{ id: 'desktop-db', parentId: null, name: 'desktop_app', kind: 'database' as const }]),
  executeQuery: vi.fn(),
  applyMutations: vi.fn(),
  saveAiKey: vi.fn(),
  askAi: vi.fn(),
});

describe('Redrob Query workspace', () => {
  it('waits for explicit desktop selection and connect before loading metadata', async () => {
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
    await user.click(screen.getByRole('button', { name: /Local Postgres disconnected/ }));
    expect(bridge.loadMetadata).not.toHaveBeenCalled();
    expect(screen.getByTestId('run-query')).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Connection actions' }));
    await user.click(screen.getByRole('button', { name: 'Connect' }));

    await waitFor(() => expect(bridge.connect).toHaveBeenCalledWith(desktopProfile.id));
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
    expect(screen.queryByText('SQL Server')).not.toBeInTheDocument();

    const name = screen.getByTestId('connection-name');
    await user.clear(name);
    await user.type(name, 'Finance replica');
    await user.click(screen.getByTestId('test-connection'));
    expect(await screen.findByText(/Demo configuration looks valid/)).toBeInTheDocument();
    await user.click(screen.getByTestId('save-connection'));
    await waitFor(() => expect(screen.queryByTestId('connection-modal')).not.toBeInTheDocument());
  });

  it('reconciles a committed save before showing its cleanup warning', async () => {
    const user = userEvent.setup();
    const bridge = new DemoBridge();
    const saveConnection = bridge.saveConnection.bind(bridge);
    vi.spyOn(bridge, 'saveConnection').mockImplementation(async (draft) => ({
      ...(await saveConnection(draft)),
      warning: 'Transaction cleanup is pending.',
    }));
    render(<App bridge={bridge} />);
    await screen.findByTestId('demo-banner');
    await user.click(screen.getByTestId('open-connection-modal'));
    await user.clear(screen.getByTestId('connection-name'));
    await user.type(screen.getByTestId('connection-name'), 'Committed profile');

    await user.click(screen.getByTestId('save-connection'));

    await waitFor(() => expect(screen.queryByTestId('connection-modal')).not.toBeInTheDocument());
    expect(await screen.findByText('Connection saved with a warning')).toBeInTheDocument();
    expect(screen.getByText('Transaction cleanup is pending.')).toBeInTheDocument();
    expect(screen.getAllByText('Committed profile').length).toBeGreaterThan(0);
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
    expect(screen.getByText(/Private and self-signed CAs are not supported in this release/i)).toBeInTheDocument();
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

  it('renders the connection-owned query language as a fixed label', async () => {
    await renderWorkspace();
    const language = screen.getByTestId('query-language');
    expect(language).toHaveTextContent('SQL');
    expect(language.tagName).toBe('SPAN');
  });

  it('opens the command palette with the keyboard shortcut', async () => {
    const user = await renderWorkspace();
    await user.keyboard('{Meta>}k{/Meta}');
    expect(screen.getByTestId('command-palette')).toBeInTheDocument();
    expect(screen.getByText('Run current query')).toBeInTheDocument();
  });
});



describe('release completion UI', () => {
  it('renders sanitized startup warnings as persistent text', async () => {
    const bridge = desktopBridge();
    vi.mocked(bridge.profileWarnings).mockResolvedValue(['Skipped\u0000 <script>alert(1)</script>']);
    render(<App bridge={bridge} />);
    const warning = await screen.findByTestId('startup-warnings');
    expect(warning).toHaveTextContent('Skipped <script>alert(1)</script>');
    expect(warning.querySelector('script')).toBeNull();
  });

  it('supports current-page controls, paging, messages, sorting, filtering, and columns', async () => {
    const user = await renderWorkspace();
    await user.click(screen.getByTestId('run-query'));
    await screen.findByText(/Showing 1–50 · more available/);
    await user.click(screen.getByRole('button', { name: 'Next page' }));
    expect(await screen.findByText(/Showing 51–64 · end of results/)).toBeInTheDocument();
    await user.selectOptions(screen.getByRole('combobox', { name: 'Rows per page' }), '25');
    expect(await screen.findByText(/Showing 1–25 · more available/)).toBeInTheDocument();

    const sortName = screen.getByRole('button', { name: 'Sort name' });
    await user.click(sortName);
    expect(screen.getByRole('button', { name: 'Sort name asc' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Columns' }));
    const email = screen.getByRole('checkbox', { name: 'email' });
    await user.click(email);
    expect(screen.queryByRole('button', { name: 'Sort email' })).not.toBeInTheDocument();

    const filter = screen.getByRole('textbox', { name: 'Filter current page' });
    await user.type(filter, 'Avery');
    expect(screen.getByRole('button', { name: 'Clear filter' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Clear filter' }));
    expect(filter).toHaveValue('');

    await user.click(screen.getByRole('button', { name: /Messages/ }));
    expect(screen.getByText('Execution messages')).toBeInTheDocument();
    expect(screen.getByText('Query completed successfully.')).toBeInTheDocument();
  });

  it('provides local history restore and clear controls', async () => {
    const user = await renderWorkspace();
    await user.click(screen.getByTestId('run-query'));
    await screen.findByText(/Showing 1–50/);
    await user.click(screen.getByRole('button', { name: 'Query history' }));
    expect(screen.getByText('Local query history')).toBeInTheDocument();
    expect(screen.getByText(/Successful first-page queries remain in memory for this browser session/)).toBeInTheDocument();
    const historyDialog = screen.getByRole('dialog', { name: 'Query history' });
    await user.click(within(historyDialog).getByRole('button', { name: /Customer overview/ }));
    expect((screen.getByTestId('monaco-editor') as HTMLTextAreaElement).value).toContain('LIMIT 250');
    await user.click(screen.getByRole('button', { name: 'Query history' }));
    await user.click(screen.getByRole('button', { name: 'Clear history' }));
    expect(screen.getByText('No successful queries yet.')).toBeInTheDocument();
  });

  it('prefills edit mode without a secret and requires confirmation before removing user profiles', async () => {
    const user = userEvent.setup();
    const bridge = new DemoBridge();
    const saveConnection = vi.spyOn(bridge, 'saveConnection');
    const removeConnection = vi.spyOn(bridge, 'removeConnection');
    const testConnection = vi.spyOn(bridge, 'testConnection');
    render(<App bridge={bridge} />);
    await screen.findByTestId('demo-banner');
    await user.click(screen.getByTestId('open-connection-modal'));
    await user.clear(screen.getByTestId('connection-name'));
    await user.type(screen.getByTestId('connection-name'), 'Temporary');
    await user.type(screen.getByTestId('connection-password'), 'never-display-this');
    await user.click(screen.getByTestId('save-connection'));
    await waitFor(() => expect(screen.queryByTestId('connection-modal')).not.toBeInTheDocument());
    await user.click(screen.getByRole('button', { name: 'Connection actions' }));
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    expect(screen.getByRole('heading', { name: 'Edit connection' })).toBeInTheDocument();
    expect(screen.getByTestId('connection-name')).toHaveValue('Temporary');
    expect(screen.getByTestId('connection-password')).toHaveValue('');
    expect(document.body).not.toHaveTextContent('never-display-this');
    await user.click(screen.getByTestId('test-connection'));
    await waitFor(() => expect(testConnection).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByTestId('test-connection')).toBeEnabled());
    expect(testConnection).toHaveBeenLastCalledWith(expect.objectContaining({ id: expect.any(String), password: '' }));
    await user.clear(screen.getByTestId('connection-host'));
    await user.type(screen.getByTestId('connection-host'), 'changed.example.test');
    await user.click(screen.getByTestId('test-connection'));
    expect(await screen.findByText(/Enter the password again before testing a changed endpoint or database identity/)).toBeInTheDocument();
    expect(testConnection).toHaveBeenCalledOnce();

    await user.clear(screen.getByTestId('connection-host'));
    await user.type(screen.getByTestId('connection-host'), 'localhost');
    await user.clear(screen.getByTestId('connection-database'));
    await user.type(screen.getByTestId('connection-database'), 'changed_database');
    await user.click(screen.getByTestId('test-connection'));
    expect(await screen.findByText(/Enter the password again before testing a changed endpoint or database identity/)).toBeInTheDocument();
    expect(testConnection).toHaveBeenCalledOnce();

    await user.clear(screen.getByTestId('connection-database'));
    await user.type(screen.getByTestId('connection-database'), 'analytics');
    await user.click(screen.getByTestId('connection-tls'));
    await user.click(screen.getByTestId('test-connection'));
    expect(await screen.findByText(/Enter the password again before testing a changed endpoint or database identity/)).toBeInTheDocument();
    expect(testConnection).toHaveBeenCalledOnce();
    await user.click(screen.getByTestId('save-connection'));
    expect(await screen.findByText(/Enter the password again after changing the endpoint or database identity/)).toBeInTheDocument();
    expect(saveConnection).toHaveBeenCalledOnce();
    await user.click(screen.getByRole('button', { name: 'Close connection manager' }));
    await user.click(screen.getByRole('button', { name: 'Connection actions' }));
    await user.click(screen.getByRole('button', { name: 'Remove' }));
    const confirmation = screen.getByRole('dialog', { name: 'Remove “Temporary”?' });
    expect(removeConnection).not.toHaveBeenCalled();
    expect(within(confirmation).getByText(/stored credential, open queries, and local query history/)).toBeInTheDocument();
    await user.click(within(confirmation).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog', { name: 'Remove “Temporary”?' })).not.toBeInTheDocument();
    expect(removeConnection).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Choose connection' })).toHaveTextContent('Temporary');

    await user.click(screen.getByRole('button', { name: 'Connection actions' }));
    await user.click(screen.getByRole('button', { name: 'Remove' }));
    await user.click(screen.getByRole('button', { name: 'Remove Temporary' }));
    await waitFor(() => expect(removeConnection).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByRole('button', { name: 'Choose connection' })).toHaveTextContent('No connection'));
    await user.click(screen.getByRole('button', { name: 'Choose connection' }));
    expect(screen.getByRole('button', { name: /Acme Warehouse connected/ })).toBeInTheDocument();
    expect(screen.queryByText('Temporary')).not.toBeInTheDocument();
  });
});



describe('built-in profile actions', () => {
  it('disables edit and remove for the seeded built-in profile', async () => {
    const user = await renderWorkspace();
    await user.click(screen.getByRole('button', { name: 'Connection actions' }));
    expect(screen.getByRole('button', { name: 'Edit' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Remove' })).toBeDisabled();
  });
});
