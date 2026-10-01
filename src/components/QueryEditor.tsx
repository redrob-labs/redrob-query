import { useState } from 'react';
import Editor from '@monaco-editor/react';
import { useGround } from '../ui/useGround';
import { Loader, Menu } from '@redrob-labs/ui';
import { Icon } from '../ui/Icon';
import clsx from 'clsx';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

const formatSql = (query: string) => query.replace(/\s+(FROM|WHERE|GROUP BY|ORDER BY|LIMIT|JOIN|LEFT JOIN|RIGHT JOIN)\s+/gi, '\n$1 ').replace(/,\s*/g, ',\n  ');
const formatMql = (query: string) => { try { return JSON.stringify(JSON.parse(query), null, 2); } catch { return query; } };

// Monaco takes a font STRING, not a CSS value, so it cannot be given `var(--font-mono)` directly.
// Reading the token off the document keeps one definition: change 30-typography.md's stack and the
// editor follows, where a stack retyped here would quietly diverge from every other monospaced surface.
const monoFontStack = () =>
  getComputedStyle(document.documentElement).getPropertyValue('--font-mono').trim() ||
  'ui-monospace, monospace';

export function QueryEditor() {
  const ground = useGround();
  const tabs = useWorkspace((state) => state.tabs);
  const activeTabId = useWorkspace((state) => state.activeTabId);
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  const visibleTabs = tabs.filter((tab) => tab.connectionId === activeTab?.connectionId);
  const status = useWorkspace((state) => state.queryStatus);
  const connections = useWorkspace((state) => state.connections);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const history = useWorkspace((state) => state.queryHistory);
  const bridgeMode = useWorkspace((state) => state.bridge.mode);
  const localPersistenceEnabled = useWorkspace((state) => state.localPersistenceEnabled);
  const localPersistenceStatus = useWorkspace((state) => state.localPersistenceStatus);
  const localPersistenceMessage = useWorkspace((state) => state.localPersistenceMessage);
  const updateQuery = useWorkspace((state) => state.updateQuery);
  const runQuery = useWorkspace((state) => state.runQuery);
  const newTab = useWorkspace((state) => state.newTab);
  const closeTab = useWorkspace((state) => state.closeTab);
  const setActiveTab = useWorkspace((state) => state.setActiveTab);
  const restoreHistory = useWorkspace((state) => state.restoreHistory);
  const clearHistory = useWorkspace((state) => state.clearHistory);
  const clearLocalWorkspace = useWorkspace((state) => state.clearLocalWorkspace);
  const enableLocalWorkspace = useWorkspace((state) => state.enableLocalWorkspace);
  const notify = useWorkspace((state) => state.notify);
  const setUi = useWorkspace((state) => state.setUi);
  const duplicateTab = useWorkspace((state) => state.duplicateTab);
  const closeOtherTabs = useWorkspace((state) => state.closeOtherTabs);
  const closeTabsToRight = useWorkspace((state) => state.closeTabsToRight);
  const [historyOpen, setHistoryOpen] = useState(false);
  const activeConnection = connections.find((connection) => connection.id === activeConnectionId);
  const connectionHistory = history.filter((entry) => entry.connectionId === activeConnectionId);
  if (!activeTab) return <section className="query-editor-shell" aria-label="Query editor"><div className="query-tabs" role="tablist" aria-label="Open queries"><IconButton label="New query requires a connection" disabled><Icon name="filePlus" /></IconButton><div className="drag-spacer" /><span className="saved-state">No active query</span></div><div className="query-toolbar"><button className="run-button" data-testid="run-query" disabled><Icon name="play" fill="currentColor" /> Run <kbd>⌘↵</kbd></button><span className="toolbar-select">No query language</span><button className="toolbar-select" disabled><Icon name="database" /> No connection</button><div className="toolbar-spacer" /><button className="ask-ai-button" onClick={() => setUi({ aiOpen: true })}><Icon name="sparkle" /> Ask Redrob</button></div><div className="editor-wrap" data-testid="query-editor"><div className="editor-loading"><Icon name="database" size={24} /> Select a connection to create a starter query.</div></div></section>;
  const copyQuery = async () => { try { await navigator.clipboard.writeText(activeTab.query); notify('success', 'Query copied'); } catch { notify('error', 'Copy unavailable', 'Select the query text and copy it manually.'); } };
  const canRun = activeConnection?.state === 'connected' && status !== 'loading';
  return (
    <section className="query-editor-shell" aria-label="Query editor">
      <div className="query-tabs" role="tablist" aria-label="Open queries">{visibleTabs.map((tab) => <div key={tab.id} className={clsx('query-tab', tab.id === activeTabId && 'is-active')}><button role="tab" aria-selected={tab.id === activeTabId} onClick={() => setActiveTab(tab.id)}><span className="sql-chip">{tab.language === 'sql' ? 'SQL' : 'M'}</span><span>{tab.name}</span>{tab.dirty ? <i className="dirty-dot" /> : null}</button><button className="tab-close" aria-label={`Close ${tab.name}`} onClick={() => closeTab(tab.id)}><Icon name="close" /></button><Menu
        label={<><Icon name="more" /><span className="sr-only">Tab actions for {tab.name}</span></>}
        variant="ghost" size="sm" align="left" className="tab-menu"
        items={[
          { id: 'duplicate', label: 'Duplicate', icon: <Icon name="duplicate" />, onSelect: () => duplicateTab(tab.id) },
          { type: 'separator' },
          { id: 'close', label: 'Close', icon: <Icon name="close" />, disabled: visibleTabs.length === 1, onSelect: () => closeTab(tab.id) },
          { id: 'close-others', label: 'Close others', icon: <Icon name="target" />, disabled: visibleTabs.length === 1, onSelect: () => closeOtherTabs(tab.id) },
          { id: 'close-right', label: 'Close tabs to the right', icon: <Icon name="chevronsRight" />, disabled: visibleTabs.at(-1)?.id === tab.id, onSelect: () => closeTabsToRight(tab.id) },
        ]} /></div>)}<IconButton label="New query" onClick={() => newTab()}><Icon name="filePlus" /></IconButton><div className="drag-spacer" /><span className="saved-state" title={bridgeMode === 'demo' ? 'Browser demo queries remain in memory for this session only.' : localPersistenceMessage ?? 'Query text remains on this device and excludes result rows, credentials, and AI prompts.'}>{localPersistenceStatus === 'error' ? <Icon name="warning" /> : <Icon name="check" />} {bridgeMode === 'demo' ? 'In-memory demo' : !localPersistenceEnabled ? 'Local save off' : localPersistenceStatus === 'error' ? 'Local save unavailable' : 'Saved locally'}</span></div>
      <div className="query-toolbar">
        <button className="run-button" data-testid="run-query" onClick={() => void runQuery()} disabled={!canRun}>{status === 'loading' ? <Loader size="sm" label="Running the query" /> : <Icon name="play" fill="currentColor" />} {status === 'loading' ? 'Running' : 'Run'} <kbd>⌘↵</kbd></button>
        <span className="toolbar-select fixed-language" data-testid="query-language"><span className="sql-chip">{activeTab.language === 'sql' ? 'SQL' : 'MQL'}</span>{activeTab.language.toUpperCase()}</span>
        <button className="toolbar-select" disabled><Icon name="database" /> {activeConnection?.name ?? 'No connection'} · {activeConnection?.state ?? 'unavailable'}</button><span className="toolbar-separator" />
        <IconButton label="Format query" onClick={() => updateQuery(activeTab.language === 'sql' ? formatSql(activeTab.query) : formatMql(activeTab.query))}><Icon name="sparkle" /></IconButton>
        <IconButton label="Copy query" onClick={() => void copyQuery()}><Icon name="copy" /></IconButton>
        <div className="history-menu"><IconButton label="Query history" onClick={() => setHistoryOpen((value) => !value)}><Icon name="clock" /></IconButton>{historyOpen ? <div className="history-popover" role="dialog" aria-label="Query history"><header><strong>Local query history</strong><button onClick={() => clearHistory(activeTab.connectionId)} disabled={!connectionHistory.length}><Icon name="trash" /> Clear history</button></header><p>{bridgeMode === 'demo' ? 'Successful first-page queries remain in memory for this browser session.' : 'Successful first-page queries and open query text are stored in plaintext on this device only; rows, credentials, and AI prompts are excluded.'}</p>{connectionHistory.length ? <div>{connectionHistory.map((entry) => <button key={entry.id} onClick={() => { restoreHistory(entry.id); setHistoryOpen(false); }}><strong>{entry.name}</strong><small>{new Date(entry.executedAt).toLocaleString()}</small><code>{entry.query}</code></button>)}</div> : <span className="history-empty">No successful queries yet.</span>}{bridgeMode === 'desktop' ? <footer>{localPersistenceEnabled ? <button onClick={() => clearLocalWorkspace()}><Icon name="trash" /> Stop saving & clear local data</button> : <button onClick={() => enableLocalWorkspace()}><Icon name="check" /> Enable local saving</button>}</footer> : null}</div> : null}</div>
        <div className="toolbar-spacer" /><button className="ask-ai-button" onClick={() => setUi({ aiOpen: true })}><Icon name="sparkle" /> Ask Redrob</button>
      </div>
      <div className="editor-wrap" data-testid="query-editor"><Editor height="100%" language={activeTab.language === 'mql' ? 'javascript' : 'sql'} value={activeTab.query} theme={ground === 'dark' ? 'vs-dark' : 'vs'} onChange={(value) => updateQuery(value ?? '')} onMount={(editor, monaco) => { editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => void runQuery()); }} options={{ automaticLayout: true, minimap: { enabled: false }, fontSize: 13, lineHeight: 21, fontFamily: monoFontStack(), fontLigatures: true, padding: { top: 12, bottom: 12 }, scrollBeyondLastLine: false, smoothScrolling: true, renderLineHighlight: 'line', overviewRulerBorder: false, hideCursorInOverviewRuler: true, folding: true, glyphMargin: false, lineNumbersMinChars: 3, wordWrap: 'off', scrollbar: { verticalScrollbarSize: 9, horizontalScrollbarSize: 9 } }} loading={<div className="editor-loading"><Loader size="md" label="Loading the editor" /> Loading editor…</div>} /></div>
    </section>
  );
}
