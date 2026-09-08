import { CheckCircle2, CloudOff, Keyboard, LockKeyhole, Sparkles } from 'lucide-react';
import { useWorkspace } from '../store/WorkspaceProvider';

export function StatusBar() {
  const mode = useWorkspace((state) => state.bridge.mode);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const connections = useWorkspace((state) => state.connections);
  const status = useWorkspace((state) => state.queryStatus);
  const result = useWorkspace((state) => state.result);
  const mutations = useWorkspace((state) => state.mutations.length);
  const active = connections.find((item) => item.id === activeConnectionId);
  return <footer className="status-bar" aria-label="Workspace status"><span className="status-brand"><Sparkles size={12} /> REDROB DATA</span><span><CheckCircle2 size={12} /> {!active ? 'No connection selected' : active.state === 'connecting' ? 'Connecting' : active.state !== 'connected' ? active.state : status === 'loading' ? 'Query running' : status === 'error' ? 'Query error' : 'Ready'}</span>{result ? <span>{result.rowCount} rows · {result.durationMs} ms</span> : null}{mutations ? <span className="pending-status">{mutations} pending</span> : null}<span className="status-spacer" /><span><LockKeyhole size={11} /> {mode === 'demo' ? 'Writes require review' : 'Results read-only'}</span><span><CloudOff size={12} /> {mode === 'demo' ? 'Browser demo' : 'Desktop local'}</span><span><Keyboard size={12} /> UTF-8</span></footer>;
}
