import { useEffect, useMemo } from 'react';
import { CheckCircle2, Info, XCircle } from 'lucide-react';
import clsx from 'clsx';
import { createBridge, type DataBridge } from './api/bridge';
import { ActivityRail } from './components/ActivityRail';
import { AiPanel } from './components/AiPanel';
import { AiSettingsModal } from './components/AiSettingsModal';
import { ChangesPanel } from './components/ChangesPanel';
import { CommandPalette } from './components/CommandPalette';
import { ConnectionModal } from './components/ConnectionModal';
import { Navigator } from './components/Navigator';
import { QueryEditor } from './components/QueryEditor';
import { ResultGrid } from './components/ResultGrid';
import { StatusBar } from './components/StatusBar';
import { WorkspaceProvider, useWorkspace } from './store/WorkspaceProvider';
import './styles/app.css';

function Workspace() {
  const initialize = useWorkspace((state) => state.initialize);
  const initialized = useWorkspace((state) => state.initialized);
  const bridgeMode = useWorkspace((state) => state.bridge.mode);
  const startupWarnings = useWorkspace((state) => state.startupWarnings);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const navigatorOpen = useWorkspace((state) => state.navigatorOpen);
  const aiOpen = useWorkspace((state) => state.aiOpen);
  const changesOpen = useWorkspace((state) => state.changesOpen);
  const setUi = useWorkspace((state) => state.setUi);
  const runQuery = useWorkspace((state) => state.runQuery);
  const newTab = useWorkspace((state) => state.newTab);
  const toasts = useWorkspace((state) => state.toasts);
  const dismissToast = useWorkspace((state) => state.dismissToast);

  useEffect(() => { void initialize(); }, [initialize]);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); setUi({ commandPaletteOpen: true }); }
      if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') { event.preventDefault(); void runQuery(); }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'n') { event.preventDefault(); newTab(); }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'i') { event.preventDefault(); setUi({ aiOpen: true }); }
      if (event.key === 'Escape') setUi({ connectionModalOpen: false, connectionModalProfileId: null, commandPaletteOpen: false, aiSettingsOpen: false });
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [newTab, runQuery, setUi]);

  useEffect(() => {
    if (!toasts.length) return;
    const timers = toasts.map((toast) => window.setTimeout(() => dismissToast(toast.id), toast.tone === 'error' ? 10_000 : 5_000));
    return () => timers.forEach((timer) => window.clearTimeout(timer));
  }, [dismissToast, toasts]);

  if (!initialized) return <div className="app-loading"><div className="loading-logo">R</div><span className="spinner" /><p>Opening your data workspace…</p></div>;
  return (
    <div className={clsx('app-shell', startupWarnings.length && 'has-startup-warnings')}>
      <div className="workspace-titlebar">
        <span className="window-dots"><i /><i /><i /></span>
        {bridgeMode === 'demo' ? <div className="demo-banner" data-testid="demo-banner"><span>DEMO</span> Interactive browser workspace · sample data only</div> : <div className="desktop-banner" data-testid="desktop-banner">Redrob Data · Read-only desktop workspace</div>}
        <button className="command-trigger" onClick={() => setUi({ commandPaletteOpen: true })}>Search or run a command <kbd>⌘ K</kbd></button>
        <span className="titlebar-mode">{bridgeMode === 'demo' ? 'No server connection' : activeConnectionId ? 'Desktop workspace' : 'No connection selected'}</span>
      </div>
      {startupWarnings.length ? <div className="startup-warnings" role="alert" data-testid="startup-warnings"><Info size={14} /><div><strong>Some local workspace items need attention</strong>{startupWarnings.map((warning, index) => <span key={`${index}-${warning}`}>{warning}</span>)}</div></div> : null}
      <div className="workspace-body">
        <ActivityRail />
        {navigatorOpen ? <Navigator /> : null}
        <main className="main-workspace">
          <QueryEditor />
          <ResultGrid />
        </main>
        {changesOpen ? <ChangesPanel /> : null}
        {aiOpen ? <AiPanel /> : null}
      </div>
      <StatusBar />
      <ConnectionModal />
      <AiSettingsModal />
      <CommandPalette />
      <div className="toast-stack" aria-live="polite">{toasts.map((toast) => <button key={toast.id} className={clsx('toast', toast.tone)} onClick={() => dismissToast(toast.id)}>{toast.tone === 'success' ? <CheckCircle2 /> : toast.tone === 'error' ? <XCircle /> : <Info />}<span><strong>{toast.title}</strong>{toast.detail ? <small>{toast.detail}</small> : null}</span></button>)}</div>
    </div>
  );
}

export function App({ bridge }: { bridge?: DataBridge }) {
  const dataBridge = useMemo(() => bridge ?? createBridge(), [bridge]);
  return <WorkspaceProvider bridge={dataBridge}><Workspace /></WorkspaceProvider>;
}

export default App;
