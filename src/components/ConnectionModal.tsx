import { useEffect, useMemo, useState } from 'react';
import { Check, ChevronRight, Database, File, Server, TestTube2, X } from 'lucide-react';
import clsx from 'clsx';
import type { ConnectionDraft, DatabaseKind } from '../domain/types';
import { useWorkspace } from '../store/WorkspaceProvider';

const databases: Array<{ kind: Exclude<DatabaseKind, 'sqlserver'>; name: string; short: string; description: string }> = [
  { kind: 'postgresql', name: 'PostgreSQL', short: 'PG', description: 'Reliable open source SQL' },
  { kind: 'mysql', name: 'MySQL', short: 'MY', description: 'Popular relational database' },
  { kind: 'sqlite', name: 'SQLite', short: 'SQ', description: 'Local embedded database' },
  { kind: 'mongodb', name: 'MongoDB', short: 'MO', description: 'Document database' },
];
const defaults: Record<Exclude<DatabaseKind, 'sqlserver'>, Pick<ConnectionDraft, 'port' | 'host'>> = {
  postgresql: { port: 5432, host: 'localhost' }, mysql: { port: 3306, host: 'localhost' }, sqlite: { port: 0, host: '' }, mongodb: { port: 27017, host: 'localhost' },
};
const freshDraft = (): ConnectionDraft => ({ name: 'Local analytics', kind: 'postgresql', host: 'localhost', port: 5432, database: 'analytics', username: 'postgres', password: '', tls: true });

export function ConnectionModal() {
  const open = useWorkspace((state) => state.connectionModalOpen);
  const profileId = useWorkspace((state) => state.connectionModalProfileId);
  const profiles = useWorkspace((state) => state.connections);
  const bridge = useWorkspace((state) => state.bridge);
  const saveConnection = useWorkspace((state) => state.saveConnection);
  const setUi = useWorkspace((state) => state.setUi);
  const [kind, setKind] = useState<Exclude<DatabaseKind, 'sqlserver'>>('postgresql');
  const [draft, setDraft] = useState<ConnectionDraft>(freshDraft);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ tone: 'success' | 'error'; message: string } | null>(null);
  const editingProfile = profiles.find((item) => item.id === profileId);
  const credentialScopeChanged = Boolean(editingProfile && (
    draft.kind !== editingProfile.kind
    || draft.host !== editingProfile.host
    || draft.port !== editingProfile.port
    || draft.database !== editingProfile.database
    || draft.username !== editingProfile.username
    || Boolean(draft.srv) !== Boolean(editingProfile.srv)
    || draft.tls !== editingProfile.tls
    || (draft.authSource ?? '') !== (editingProfile.authSource ?? '')
  ));
  const replacementSecretRequired = credentialScopeChanged && draft.kind !== 'sqlite' && !draft.password?.trim();
  useEffect(() => {
    if (!open) return;
    if (editingProfile && editingProfile.kind !== 'sqlserver') {
      setKind(editingProfile.kind);
      const { state: _state, builtIn: _builtIn, isDemo: _isDemo, ...profile } = editingProfile;
      setDraft({ ...profile, password: '' });
    } else {
      const initial = freshDraft(); setKind('postgresql'); setDraft(initial);
    }
    setFeedback(null);
  }, [editingProfile, open]);
  const selected = useMemo(() => databases.find((item) => item.kind === kind)!, [kind]);
  if (!open) return null;
  const close = () => setUi({ connectionModalOpen: false, connectionModalProfileId: null });
  const update = (key: keyof ConnectionDraft, value: string | number | boolean) => setDraft((current) => ({ ...current, [key]: value }));
  const chooseKind = (nextKind: Exclude<DatabaseKind, 'sqlserver'>) => {
    if (editingProfile && nextKind !== editingProfile.kind) return;
    setKind(nextKind); setFeedback(null);
    setDraft((current) => ({ ...current, kind: nextKind, ...defaults[nextKind], password: nextKind === 'sqlite' ? '' : current.password, username: nextKind === 'sqlite' ? '' : current.username, srv: false, tls: true, authSource: nextKind === 'mongodb' ? (current.authSource || 'admin') : undefined, database: nextKind === 'sqlite' ? '' : current.database }));
  };
  const test = async () => {
    if (replacementSecretRequired) { setFeedback({ tone: 'error', message: 'Enter the password again before testing a changed endpoint or database identity.' }); return; }
    setTesting(true); setFeedback(null);
    try { const result = await bridge.testConnection(draft); const latency = result.ok && result.latencyMs !== undefined ? ` · ${result.latencyMs} ms` : ''; setFeedback({ tone: result.ok ? 'success' : 'error', message: `${result.message}${latency}` }); }
    catch (error) { setFeedback({ tone: 'error', message: error instanceof Error ? error.message : 'Connection test failed.' }); }
    finally { setTesting(false); }
  };
  const save = async () => {
    if (replacementSecretRequired) { setFeedback({ tone: 'error', message: 'Enter the password again after changing the endpoint or database identity.' }); return; }
    setSaving(true); setFeedback(null);
    try { await saveConnection(draft); }
    catch (error) { setFeedback({ tone: 'error', message: error instanceof Error ? error.message : 'Could not save connection.' }); }
    finally { setSaving(false); }
  };
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) close(); }}>
      <section className="connection-modal" role="dialog" aria-modal="true" aria-labelledby="connection-title" data-testid="connection-modal" onKeyDown={(event) => {
        if (event.key === 'Escape') close();
        if (event.key !== 'Tab') return;
        const controls = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), [tabindex="0"]'));
        const first = controls[0]; const last = controls.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }}>
        <div className="modal-heading"><div><span className="modal-icon"><Database size={18} /></span><div><h2 id="connection-title">{editingProfile ? 'Edit connection' : 'New connection'}</h2><p>{editingProfile ? 'Update this profile without exposing its stored secret' : 'Connect a database to your workspace'}</p></div></div><button aria-label="Close connection manager" onClick={close}><X size={18} /></button></div>
        <div className="connection-body">
          <div className="database-list"><span className="field-label">Database type</span>{databases.map((database) => <button key={database.kind} className={clsx(kind === database.kind && 'is-active')} disabled={Boolean(editingProfile && database.kind !== editingProfile.kind)} title={editingProfile && database.kind !== editingProfile.kind ? 'Create a new profile to change database type' : undefined} onClick={() => chooseKind(database.kind)} data-testid={`database-${database.kind}`}><span className={`db-logo ${database.kind}`}>{database.short}</span><span><strong>{database.name}</strong><small>{database.description}</small></span><ChevronRight size={14} /></button>)}</div>
          <form className="connection-form" onSubmit={(event) => { event.preventDefault(); void save(); }}>
            <div className="form-title"><span className={`db-logo large ${kind}`}>{selected.short}</span><div><h3>{selected.name}</h3><p>{bridge.mode === 'demo' ? 'Saved inside this browser demo only' : 'Credentials are stored securely on this device'}</p></div></div>
            <label><span>Connection name</span><input data-testid="connection-name" autoFocus value={draft.name} onChange={(event) => update('name', event.target.value)} placeholder="Production warehouse" required /></label>
            {kind === 'sqlite' ? <label><span>Database file</span><div className="input-icon"><File size={14} /><input value={draft.filePath ?? ''} onChange={(event) => update('filePath', event.target.value)} placeholder="/path/to/database.sqlite" /></div></label> : <>
              <div className="field-row"><label className="grow"><span>Host</span><div className="input-icon"><Server size={14} /><input data-testid="connection-host" value={draft.host} onChange={(event) => update('host', event.target.value)} required /></div></label><label className="port"><span>Port</span><input type="number" value={draft.port} disabled={kind === 'mongodb' && Boolean(draft.srv)} onChange={(event) => update('port', Number(event.target.value))} /></label></div>
              <label><span>Database</span><input data-testid="connection-database" value={draft.database} onChange={(event) => update('database', event.target.value)} placeholder="analytics" /></label>
              {kind === 'mongodb' ? <label><span>Authentication database</span><input data-testid="connection-auth-source" value={draft.authSource ?? ''} onChange={(event) => update('authSource', event.target.value)} placeholder="admin" /></label> : null}
              <div className="field-row"><label><span>Username</span><input data-testid="connection-username" value={draft.username} onChange={(event) => update('username', event.target.value)} /></label><label><span>Password</span><input data-testid="connection-password" type="password" value={draft.password ?? ''} onChange={(event) => update('password', event.target.value)} placeholder={editingProfile ? credentialScopeChanged ? 'Required after endpoint or identity change' : 'Leave blank to keep stored secret' : '••••••••'} required={replacementSecretRequired} /></label></div>
            </>}
            {kind === 'mongodb' ? <label className="checkbox-label"><input type="checkbox" checked={Boolean(draft.srv)} onChange={(event) => setDraft((current) => ({ ...current, srv: event.target.checked, port: event.target.checked ? 0 : 27017 }))} /><span><Check size={11} /></span>Use DNS seed list (mongodb+srv)</label> : null}
            {kind !== 'sqlite' ? <><label className="checkbox-label"><input data-testid="connection-tls" type="checkbox" checked={draft.tls} onChange={(event) => update('tls', event.target.checked)} /><span><Check size={11} /></span>Require identity-verifying TLS</label><p className="field-hint">Certificate and hostname verification use publicly trusted CAs. Private and self-signed CAs are not supported in this release.</p></> : null}
            {feedback ? <div className={`connection-feedback ${feedback.tone}`} role="status">{feedback.tone === 'success' ? <Check size={14} /> : <X size={14} />}{feedback.message}</div> : null}
          </form>
        </div>
        <div className="modal-footer"><span><i /> {bridge.mode === 'demo' ? 'Demo mode · no network request is made' : editingProfile ? credentialScopeChanged ? 'Endpoint or identity changed · enter the password again' : 'Blank password uses the stored secret for testing and keeps it unchanged on save' : 'Desktop mode · credentials use secure local storage'}</span><div><button className="ghost-button" data-testid="test-connection" onClick={() => void test()} disabled={testing}>{testing ? <span className="spinner small" /> : <TestTube2 size={14} />} {testing ? 'Testing…' : 'Test connection'}</button><button className="primary-button" data-testid="save-connection" onClick={() => void save()} disabled={saving}>{saving ? <span className="spinner small" /> : null}{saving ? 'Saving…' : editingProfile ? 'Save changes' : 'Save connection'}</button></div></div>
      </section>
    </div>
  );
}
