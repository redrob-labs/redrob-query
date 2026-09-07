import Editor from '@monaco-editor/react';
import { Check, ChevronDown, Clock3, Copy, DatabaseZap, FilePlus2, MoreHorizontal, Play, Save, Sparkles, X } from 'lucide-react';
import clsx from 'clsx';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

const formatSql = (query: string) => query.replace(/\s+(FROM|WHERE|GROUP BY|ORDER BY|LIMIT|JOIN|LEFT JOIN|RIGHT JOIN)\s+/gi, '\n$1 ').replace(/,\s*/g, ',\n  ');

export function QueryEditor() {
  const tabs = useWorkspace((state) => state.tabs);
  const activeTabId = useWorkspace((state) => state.activeTabId);
  const activeTab = tabs.find((tab) => tab.id === activeTabId)!;
  const visibleTabs = tabs.filter((tab) => tab.connectionId === activeTab?.connectionId);
  const status = useWorkspace((state) => state.queryStatus);
  const connections = useWorkspace((state) => state.connections);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const updateQuery = useWorkspace((state) => state.updateQuery);
  const setTabLanguage = useWorkspace((state) => state.setTabLanguage);
  const runQuery = useWorkspace((state) => state.runQuery);
  const newTab = useWorkspace((state) => state.newTab);
  const closeTab = useWorkspace((state) => state.closeTab);
  const setActiveTab = useWorkspace((state) => state.setActiveTab);
  const notify = useWorkspace((state) => state.notify);
  const setUi = useWorkspace((state) => state.setUi);
  const activeConnection = connections.find((connection) => connection.id === activeConnectionId);
  if (!activeTab) {
    return (
      <section className="query-editor-shell" aria-label="Query editor">
        <div className="query-tabs" role="tablist" aria-label="Open queries">
          <IconButton label="New query requires a connection" disabled><FilePlus2 size={15} /></IconButton>
          <div className="drag-spacer" />
          <span className="saved-state">No active query</span>
        </div>
        <div className="query-toolbar">
          <button className="run-button" data-testid="run-query" disabled><Play size={14} fill="currentColor" /> Run <kbd>⌘↵</kbd></button>
          <button className="toolbar-select" disabled>No query language</button>
          <button className="toolbar-select" disabled><DatabaseZap size={14} /> No connection</button>
          <div className="toolbar-spacer" />
          <button className="ask-ai-button" onClick={() => setUi({ aiOpen: true })}><Sparkles size={14} /> Ask Redrob</button>
        </div>
        <div className="editor-wrap" data-testid="query-editor">
          <div className="editor-loading"><DatabaseZap size={18} /> Select a connection to create a starter query.</div>
        </div>
      </section>
    );
  }
  const copyQuery = async () => {
    try { await navigator.clipboard.writeText(activeTab.query); notify('success', 'Query copied'); }
    catch { notify('error', 'Copy unavailable', 'Select the query text and copy it manually.'); }
  };
  return (
    <section className="query-editor-shell" aria-label="Query editor">
      <div className="query-tabs" role="tablist" aria-label="Open queries">
        {visibleTabs.map((tab) => <div key={tab.id} className={clsx('query-tab', tab.id === activeTabId && 'is-active')}>
          <button role="tab" aria-selected={tab.id === activeTabId} onClick={() => setActiveTab(tab.id)}><span className="sql-chip">{tab.language === 'sql' ? 'SQL' : 'M'}</span><span>{tab.name}</span>{tab.dirty ? <i className="dirty-dot" /> : null}</button>
          <button className="tab-close" aria-label={`Close ${tab.name}`} onClick={() => closeTab(tab.id)}><X size={12} /></button>
        </div>)}
        <IconButton label="New query" onClick={() => newTab()}><FilePlus2 size={15} /></IconButton><div className="drag-spacer" /><span className="saved-state"><Check size={12} /> Local</span>
      </div>
      <div className="query-toolbar">
        <button className="run-button" data-testid="run-query" onClick={() => void runQuery()} disabled={status === 'loading'}>{status === 'loading' ? <span className="spinner small" /> : <Play size={14} fill="currentColor" />} {status === 'loading' ? 'Running' : 'Run'} <kbd>⌘↵</kbd></button>
        <button className="toolbar-select" data-testid="query-language" onClick={() => setTabLanguage(activeTab.language === 'sql' ? 'mql' : 'sql')} aria-label="Switch query language"><span className="sql-chip">{activeTab.language === 'sql' ? 'SQL' : 'MQL'}</span>{activeTab.language.toUpperCase()}<ChevronDown size={12} /></button>
        <button className="toolbar-select" disabled><DatabaseZap size={14} /> {activeConnection?.name ?? 'No connection'}</button><span className="toolbar-separator" />
        <IconButton label="Format query" onClick={() => updateQuery(activeTab.language === 'sql' ? formatSql(activeTab.query) : activeTab.query)}><Sparkles size={15} /></IconButton>
        <IconButton label="Save is automatic" disabled><Save size={15} /></IconButton><IconButton label="Copy query" onClick={() => void copyQuery()}><Copy size={15} /></IconButton><IconButton label="Query history unavailable" disabled><Clock3 size={15} /></IconButton>
        <div className="toolbar-spacer" /><button className="ask-ai-button" onClick={() => setUi({ aiOpen: true })}><Sparkles size={14} /> Ask Redrob</button><IconButton label="More query actions unavailable" disabled><MoreHorizontal size={16} /></IconButton>
      </div>
      <div className="editor-wrap" data-testid="query-editor">
        <Editor height="100%" language={activeTab.language === 'mql' ? 'javascript' : 'sql'} value={activeTab.query} theme="vs-dark" onChange={(value) => updateQuery(value ?? '')} onMount={(editor, monaco) => { editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => void runQuery()); }} options={{ automaticLayout: true, minimap: { enabled: false }, fontSize: 13, lineHeight: 21, fontFamily: "'SFMono-Regular', 'Cascadia Code', Consolas, monospace", fontLigatures: true, padding: { top: 12, bottom: 12 }, scrollBeyondLastLine: false, smoothScrolling: true, renderLineHighlight: 'line', overviewRulerBorder: false, hideCursorInOverviewRuler: true, folding: true, glyphMargin: false, lineNumbersMinChars: 3, wordWrap: 'off', scrollbar: { verticalScrollbarSize: 9, horizontalScrollbarSize: 9 } }} loading={<div className="editor-loading"><span className="spinner" /> Loading editor…</div>} />
      </div>
    </section>
  );
}
