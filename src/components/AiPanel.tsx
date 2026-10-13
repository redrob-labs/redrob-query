import { useEffect, useRef, useState } from 'react';
import { Icon } from '../ui/Icon';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

export function AiPanel() {
  const messages = useWorkspace((state) => state.aiMessages);
  const status = useWorkspace((state) => state.aiStatus);
  const mode = useWorkspace((state) => state.bridge.mode);
  const width = useWorkspace((state) => state.aiWidth);
  const connections = useWorkspace((state) => state.connections);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const tabs = useWorkspace((state) => state.tabs);
  const activeTabId = useWorkspace((state) => state.activeTabId);
  const askAi = useWorkspace((state) => state.askAi);
  const useQuery = useWorkspace((state) => state.useGeneratedQuery);
  const setUi = useWorkspace((state) => state.setUi);
  const [prompt, setPrompt] = useState('');
  const messagesRef = useRef<HTMLDivElement>(null);
  useEffect(() => { messagesRef.current?.scrollTo({ top: messagesRef.current.scrollHeight, behavior: 'smooth' }); }, [messages, status]);
  const send = async () => {
    if (!prompt.trim() || status === 'loading') return;
    const value = prompt;
    setPrompt('');
    await askAi(value);
  };
  const activeConnection = connections.find((connection) => connection.id === activeConnectionId);
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  const resize = (next: number) => setUi({ aiWidth: Math.max(290, Math.min(480, next)) });
  return (
    <aside className="ai-panel" style={{ width }} aria-label="Redrob AI assistant" data-testid="ai-panel">
      <div className="resize-handle" role="separator" tabIndex={0} aria-label="Resize AI panel" aria-orientation="vertical" aria-valuemin={290} aria-valuemax={480} aria-valuenow={width} onKeyDown={(event) => {
        if (event.key === 'ArrowLeft') resize(width + 12);
        if (event.key === 'ArrowRight') resize(width - 12);
      }} onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        const startX = event.clientX;
        const startWidth = width;
        const move = (moveEvent: PointerEvent) => resize(startWidth + startX - moveEvent.clientX);
        const up = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up); };
        window.addEventListener('pointermove', move); window.addEventListener('pointerup', up);
      }} />
      <div className="ai-heading"><div className="ai-brand"><span><Icon name="sparkle" /></span><div><strong>Redrob AI</strong><small><i /> Context aware</small></div></div><div className="ai-heading-actions"><IconButton label="Redrob settings" data-testid="open-ai-settings-header" onClick={() => setUi({ aiSettingsOpen: true })}><Icon name="settings" /></IconButton><IconButton label="Close AI panel" onClick={() => setUi({ aiOpen: false })}><Icon name="panelRight" /></IconButton></div></div>
      <div className="ai-context"><DatabaseContext kind={activeConnection?.kind} /><span>{activeConnection?.name ?? 'No connection'}</span><span className="context-separator">/</span><Icon name="code" /><span>{activeTab?.name ?? 'No query'}</span></div>
      <div className="ai-messages" ref={messagesRef} aria-live="polite">
        <div className="ai-welcome"><span><Icon name="sparkle" size={24} /></span><h2>Build queries for this engine</h2><p>{mode === 'desktop' ? 'Your prompt and active query are sent to Redrob. Database result rows and credentials are not automatically attached. The desktop app trusts user-entered content in those fields, so anything you include there is sent.' : 'Demo AI runs locally with your prompt and active query. Result rows and credentials are not used.'}</p></div>
        {messages.map((message, index) => <div key={`${message.role}-${index}`} className={`ai-message ${message.role}`}>
          {message.role === 'assistant' ? <span className="bot-avatar"><Icon name="sparkle" /></span> : null}
          <div className="message-content"><p>{message.content}</p>{message.query ? <div className="generated-query"><div><Icon name="code" /> Generated {message.language === 'mql' ? 'MQL' : 'SQL'}</div><pre>{message.query}</pre><button data-testid="use-ai-query" onClick={() => useQuery(message.query!)}><Icon name="edit" /> Use in editor</button></div> : null}</div>
        </div>)}
        {status === 'loading' ? <div className="ai-message assistant"><span className="bot-avatar"><Icon name="sparkle" /></span><div className="ai-thinking"><i /><i /><i /><span>Thinking with engine and active-query context</span></div></div> : null}
      </div>
      <div className="suggestion-row"><button onClick={() => setPrompt(activeConnection?.kind === 'mongodb' ? 'Find customer documents' : 'Show monthly revenue')}><Icon name="sparkle" /> {activeConnection?.kind === 'mongodb' ? 'Customer find' : 'Revenue query'}</button><button onClick={() => setPrompt('Explain the active query')}><Icon name="lightbulb" /> Explain query</button></div>
      <div className="ai-composer">
        <textarea data-testid="ai-input" value={prompt} onChange={(event) => setPrompt(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void send(); } }} placeholder="Ask about your data…" aria-label="Message Redrob AI" rows={2} />
        <div><span>AI can make mistakes · <kbd>↵</kbd> send</span><button data-testid="send-ai" aria-label="Send message" onClick={() => void send()} disabled={!prompt.trim() || status === 'loading'}><Icon name="arrowUp" /></button></div>
      </div>
    </aside>
  );
}

function DatabaseContext({ kind }: { kind?: 'postgresql' | 'mysql' | 'sqlite' | 'mongodb' | 'sqlserver' }) {
  const labels = { postgresql: 'PG', mysql: 'MY', sqlite: 'SQ', mongodb: 'MO', sqlserver: 'MS' } as const;
  return <span className="context-db">{kind ? labels[kind] : 'DB'}</span>;
}
