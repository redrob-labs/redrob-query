import { useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, Circle, Columns3, Database, Edit3, Eye, Link, Link2Off, MoreHorizontal, Plus, RefreshCw, Search, Table2, Trash2 } from 'lucide-react';
import clsx from 'clsx';
import type { ConnectionProfile, DatabaseKind, MetadataNode } from '../domain/types';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

const engineLabel: Record<DatabaseKind, string> = { postgresql: 'PostgreSQL', mysql: 'MySQL', sqlite: 'SQLite', mongodb: 'MongoDB', sqlserver: 'SQL Server' };
const engineGlyph: Record<DatabaseKind, string> = { postgresql: 'PG', mysql: 'MY', sqlite: 'SQ', mongodb: 'MO', sqlserver: 'MS' };
const NodeIcon = ({ node }: { node: MetadataNode }) => node.kind === 'database' ? <Database size={14} /> : node.kind === 'table' ? <Table2 size={14} className="node-icon table" /> : node.kind === 'view' ? <Eye size={14} className="node-icon view" /> : node.kind === 'column' ? <Columns3 size={13} className="node-icon column" /> : <span className="schema-icon">S</span>;

function TreeNode({ node, depth, filter }: { node: MetadataNode; depth: number; filter: string }) {
  const expanded = useWorkspace((state) => state.expandedNodes.has(node.id));
  const children = useWorkspace((state) => state.metadata[node.id]);
  const toggleNode = useWorkspace((state) => state.toggleNode);
  const hasChildren = Boolean(node.childCount);
  const visibleChildren = children?.filter((child) => !filter || child.name.toLowerCase().includes(filter) || Boolean(child.childCount));
  if (filter && !hasChildren && !node.name.toLowerCase().includes(filter)) return null;
  return <><button className={clsx('tree-node', node.kind === 'column' && 'is-column')} style={{ paddingLeft: 10 + depth * 14 }} onClick={() => hasChildren && void toggleNode(node)} aria-expanded={hasChildren ? expanded : undefined} data-testid={`metadata-${node.id}`}><span className="tree-chevron">{hasChildren ? (expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />) : <span />}</span><NodeIcon node={node} /><span className="tree-label">{node.name}</span>{node.dataType ? <span className="tree-type">{node.dataType}</span> : null}</button>{expanded && !children ? <div className="tree-loading" style={{ paddingLeft: 32 + depth * 14 }}><Circle size={8} /> Loading…</div> : null}{expanded && visibleChildren?.map((child) => <TreeNode key={child.id} node={child} depth={depth + 1} filter={filter} />)}</>;
}

export function Navigator() {
  const connections = useWorkspace((state) => state.connections);
  const activeId = useWorkspace((state) => state.activeConnectionId);
  const roots = useWorkspace((state) => state.metadata.root);
  const setActive = useWorkspace((state) => state.setActiveConnection);
  const openConnectionModal = useWorkspace((state) => state.openConnectionModal);
  const connectConnection = useWorkspace((state) => state.connectConnection);
  const disconnectConnection = useWorkspace((state) => state.disconnectConnection);
  const removeConnection = useWorkspace((state) => state.removeConnection);
  const loadNode = useWorkspace((state) => state.loadNode);
  const searchRequest = useWorkspace((state) => state.navigatorSearchRequest);
  const searchRef = useRef<HTMLInputElement>(null);
  const [filter, setFilter] = useState('');
  const [pickerOpen, setPickerOpen] = useState(false);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [pendingRemoval, setPendingRemoval] = useState<ConnectionProfile | null>(null);
  const [removing, setRemoving] = useState(false);
  useEffect(() => { if (searchRequest > 0) { searchRef.current?.focus(); searchRef.current?.select(); } }, [searchRequest]);
  const active = connections.find((item) => item.id === activeId);
  const closeRemoval = () => { if (!removing) setPendingRemoval(null); };
  const confirmRemoval = async () => {
    if (!pendingRemoval || removing) return;
    const profileId = pendingRemoval.id;
    setRemoving(true);
    await removeConnection(profileId);
    setRemoving(false);
    setPendingRemoval(null);
  };
  return (
    <aside className="navigator" aria-label="Connection and schema navigator" data-testid="navigator">
      <div className="panel-heading"><div><span className="eyebrow">Workspace</span><h1>Data explorer</h1></div><IconButton label="Add connection" onClick={() => openConnectionModal()}><Plus size={16} /></IconButton></div>
      <label className="navigator-search"><Search size={14} /><span className="sr-only">Filter schemas</span><input ref={searchRef} value={filter} onChange={(event) => setFilter(event.target.value.toLowerCase())} placeholder="Filter schemas…" /></label>
      <div className="connection-picker">
        <button className="connection-current" aria-label="Choose connection" aria-expanded={pickerOpen} onClick={() => setPickerOpen((value) => !value)}><span className="database-glyph">{active ? engineGlyph[active.kind] : 'DB'}</span><span><strong>{active?.name ?? 'No connection'}</strong><small>{active ? <i className={`state-${active.state}`} /> : null} {active?.isDemo ? 'Demo ' : ''}{active ? `${engineLabel[active.kind]} · ${active.state}` : 'Select a profile'}</small></span><ChevronDown size={14} /></button>
        {pickerOpen && connections.length > 0 ? <div className="connection-options">{connections.map((connection) => <button key={connection.id} onClick={() => { setPickerOpen(false); void setActive(connection.id); }}><strong>{connection.name}</strong><small>{connection.state}</small></button>)}</div> : null}
      </div>
      <div className="tree-toolbar"><span>Objects</span><div><IconButton label="Refresh schema" disabled={!active || active.state !== 'connected'} onClick={() => void loadNode(null, true)}><RefreshCw size={14} /></IconButton><div className="profile-actions"><IconButton label="Connection actions" disabled={!active} onClick={() => setActionsOpen((value) => !value)}><MoreHorizontal size={15} /></IconButton>{actionsOpen && active ? <div className="profile-actions-menu" role="menu"><button disabled={Boolean(active.builtIn) || active.state === 'connecting'} title={active.builtIn ? 'Built-in profiles cannot be edited' : active.state === 'connecting' ? 'Wait for the connection attempt to finish' : undefined} onClick={() => { setActionsOpen(false); openConnectionModal(active.id); }}><Edit3 size={13} /> Edit</button>{active.state === 'connected' ? <button onClick={() => { setActionsOpen(false); void disconnectConnection(active.id); }}><Link2Off size={13} /> Disconnect</button> : <button disabled={active.state === 'connecting'} onClick={() => { setActionsOpen(false); void connectConnection(active.id); }}><Link size={13} /> {active.state === 'connecting' ? 'Connecting…' : 'Connect'}</button>}<button className="danger" disabled={Boolean(active.builtIn) || active.state === 'connecting'} title={active.builtIn ? 'Built-in profiles cannot be removed' : active.state === 'connecting' ? 'Wait for the connection attempt to finish' : undefined} onClick={() => { setActionsOpen(false); setPendingRemoval(active); }}><Trash2 size={13} /> Remove</button></div> : null}</div></div></div>
      <div className="schema-tree" role="tree" aria-label="Database objects">{!active ? <div className="navigator-state">Select a connection to view its status.</div> : active.state !== 'connected' ? <div className="navigator-state">{active.state === 'connecting' ? <><span className="spinner" /> Connecting…</> : 'Connect this profile to load its schema.'}</div> : !roots ? <div className="navigator-state"><span className="spinner" /> Loading schema…</div> : roots.length === 0 ? <div className="navigator-state">No objects found</div> : roots.map((node) => <TreeNode key={node.id} node={node} depth={0} filter={filter} />)}</div>
      {active?.state === 'connected' && roots ? <div className="navigator-footer"><span className="live-dot" /> Metadata synced <span>just now</span></div> : <div className="navigator-footer">{active ? `${active.state[0].toUpperCase()}${active.state.slice(1)}` : 'Waiting for connection selection'}</div>}
      {pendingRemoval ? <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) closeRemoval(); }}><section className="remove-confirmation" role="dialog" aria-modal="true" aria-labelledby="remove-connection-title" aria-describedby="remove-connection-description" onKeyDown={(event) => {
        if (event.key === 'Escape') closeRemoval();
        if (event.key !== 'Tab') return;
        const controls = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), [tabindex="0"]'));
        const first = controls[0]; const last = controls.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }}><div className="remove-confirmation-icon"><Trash2 size={20} /></div><h2 id="remove-connection-title">Remove “{pendingRemoval.name}”?</h2><p id="remove-connection-description">This permanently removes the saved profile, its stored credential, open queries, and local query history for this connection.</p><div><button className="ghost-button" autoFocus onClick={closeRemoval} disabled={removing}>Cancel</button><button className="danger-button" onClick={() => void confirmRemoval()} disabled={removing}>{removing ? <span className="spinner small" /> : <Trash2 size={14} />} {removing ? 'Removing…' : `Remove ${pendingRemoval.name}`}</button></div></section></div> : null}
    </aside>
  );
}
